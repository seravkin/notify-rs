use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use chrono::Utc;
use fnv::FnvHashMap;
use crate::db::{EventRepository, UserRepository};
use crate::errors::BotError;
use crate::models::{Env, InlineKeyboardButton, InlineKeyboardMarkup, Message, Notification, Update};
use crate::parser::OpenAIParser;
use crate::tg::Tg;
use std::fmt::Write;
use log::{error, info};
use tokio::task::JoinHandle;


#[derive(Debug, Clone)]
pub enum State {
    Idle,
    Parsed { text: String, notification: Notification },
    ParsedWithError { text: String }
}

pub struct BotDeps {
    event_repository: EventRepository,
    user_repository: UserRepository,
    parser: OpenAIParser,
    tg: Tg
}

impl BotDeps {
    pub async fn new(env: &Env) -> Result<BotDeps, BotError> {
        let event_repository = EventRepository::new(&env.connection_string).await?;
        let user_repository = UserRepository::new(env.user_ids.iter().copied());
        let parser = OpenAIParser::new(env.openai_token.to_string());
        let tg = Tg::new(env.bot_token.to_string());
        Ok(BotDeps { user_repository, event_repository, parser, tg })
    }
}

impl BotHandler {
    async fn handle_message(&self, message: Message) -> Result<(), BotError> {
        if let Some(text) = message.text {
            let result = self.bot.parser.parse(Utc::now(), text.as_str()).await;
            let (text, state) = match result {
                Ok(notification) =>
                    (serde_json::to_string(&notification)?, State::Parsed { text: text.clone(), notification }),
                Err(error) =>
                    (format!("{}", error), State::ParsedWithError { text })
            };
            let markup = InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![InlineKeyboardButton {
                        text: "Accept".to_string(),
                        callback_data: CallbackQuery::Accept.to_string()
                    }],
                    vec![InlineKeyboardButton {
                        text: "Repeat".to_string(),
                        callback_data: CallbackQuery::Repeat.to_string()
                    }],
                    vec![InlineKeyboardButton {
                        text: "Cancel".to_string(),
                        callback_data: CallbackQuery::Cancel.to_string()
                    }]
                ]
            };
            self.bot.tg.send_message(message.chat.id, text, Some(markup)).await?;
            self.state_channel.send((message.chat.id, state))?;
        }

        Ok(())
    }

    async fn handle_update(&self, update: Update) -> Result<(), BotError> {
        if let Some(callback_query) = update.callback_query {
            self.handle_callback_query(callback_query).await
        } else if let Some(message) = update.message {
            self.handle_message(message).await
        } else {
            Ok(())
        }
    }

    async fn handle_callback_query(&self, callback_query: crate::models::CallbackQuery) -> Result<(), BotError> {
        let data: CallbackQuery = callback_query.data.as_ref().ok_or(BotError::InvalidCallbackQuery)?.parse::<CallbackQuery>()?;
        let chat_id = callback_query.from.id;
        info!("{:?}, {:?}", self.state, data);
        let (answer_text, new_state) = match (self.state.clone(), data) {
            (_, CallbackQuery::Cancel) => {
                self.cancel(&callback_query).await?
            },
            (State::ParsedWithError { text}, CallbackQuery::Repeat) => {
                self.repeat(&callback_query, &text).await?
            },
            (state @ State::ParsedWithError { .. }, CallbackQuery::Accept) => {
                (Some("Impossible to accept notification with errors".to_string()), state)
            },
            (State::Parsed { notification, .. }, CallbackQuery::Accept) => {
                self.accept(&callback_query, notification).await?
            },
            (State::Parsed { text, .. }, CallbackQuery::Repeat) => {
                self.repeat(&callback_query, &text).await?
            },
            (state, CallbackQuery::Delete(ids)) => {
                self.bot.event_repository.delete_events(ids).await?;
                self.bot.tg.delete_message(
                    callback_query.from.id,
                    callback_query.message
                        .ok_or(BotError::InvalidCallbackQuery)?
                        .message_id
                ).await?;
                (Some("Notification deleted".to_string()), state)
            }
            (state, _) => (None, state)
        };

        self.state_channel.send((chat_id, new_state))?;
        self.bot.tg.answer_callback_query(callback_query.id.clone(), answer_text).await?;
        Ok(())
    }

    async fn accept(&self, callback_query: &crate::models::CallbackQuery, notification: Notification) -> Result<(Option<String>, State), BotError> {
        let as_json = serde_json::to_string(&notification)?;
        let new_text = format!("Response: {}", as_json);
        let ids = self.bot.event_repository.insert_event(callback_query.from.id,  notification.get_text().to_string(), notification.create_stored_notifications(Utc::now())).await?;
        info!("{:?}", ids);
        let message = callback_query.message.as_ref().ok_or(BotError::InvalidCallbackQuery)?;
        self.bot.tg.edit_message_text(message.chat.id, message.message_id, new_text, Some(InlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![
                    InlineKeyboardButton {
                        text: "Cancel".to_string(),
                        callback_data: CallbackQuery::Delete(ids).to_string()
                    }
                ]
            ]
        })).await?;

        Ok((Some("Notification accepted".to_string()), State::Idle))
    }

    async fn repeat(&self, callback_query: &crate::models::CallbackQuery, text: &String) -> Result<(Option<String>, State), BotError> {
        let result = self.bot.parser.parse(Utc::now(), &text).await;
        match result {
            Ok(result) => {
                let message = callback_query.message.as_ref().ok_or(BotError::InvalidCallbackQuery)?;
                let as_json = serde_json::to_string(&result)?;
                let new_text = format!("Response: {}", as_json);
                self.bot.tg.edit_message_text(message.chat.id, message.message_id, new_text, None).await?;
                Ok((Some("Request was repeated".to_string()), State::Parsed { text: text.clone(), notification: result }))
            }
            Err(err) => {
                let new_text = format!("Error: {}", err);
                let message = callback_query.message.as_ref().ok_or(BotError::InvalidCallbackQuery)?;
                self.bot.tg.edit_message_text(message.chat.id, message.message_id, new_text, None).await?;
                Ok((Some("Error while parsing command".to_string()), State::ParsedWithError { text: text.clone() }))
            }
        }
    }

    async fn cancel(&self, callback_query: &crate::models::CallbackQuery) -> Result<(Option<String>, State), BotError> {
        self.bot.tg.delete_message( callback_query.from.id,
                                callback_query.message.as_ref().ok_or(BotError::InvalidCallbackQuery)?.message_id).await?;
        Ok((Some("Canceled".to_string()), State::Idle))
    }
}


pub struct Bot {
    pub dependency: Arc<BotDeps>
}

pub struct BotHandler {
    bot: Arc<BotDeps>,
    state: State,
    state_channel: tokio::sync::mpsc::UnboundedSender<(u64, State)>
}

#[derive(Debug)]
enum CallbackQuery {
    Repeat, Accept, Cancel, Delete(Vec<u64>)
}

impl FromStr for CallbackQuery {
    type Err = BotError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "repeat" => Ok(CallbackQuery::Repeat),
            "accept" => Ok(CallbackQuery::Accept),
            "cancel" => Ok(CallbackQuery::Cancel),
            _ => {
                let ids: Result<Vec<u64>, _> = s.split(',').map(|s| u64::from_str(s).map_err(|_| BotError::InvalidCallbackQuery)).collect();
                Ok(CallbackQuery::Delete(ids?))
            }
        }
    }
}

impl ToString for CallbackQuery {
    fn to_string(&self) -> String {
        match self {
            CallbackQuery::Repeat => "repeat".to_string(),
            CallbackQuery::Accept => "accept".to_string(),
            CallbackQuery::Cancel => "cancel".to_string(),
            CallbackQuery::Delete(ids) => {
                // write ids as string separated by comma with only one allocation
                let mut s = String::with_capacity(ids.len() * 10);
                for id in ids {
                    let _ = write!(s, "{},", id);
                }
                if s.ends_with(",") {
                    s.pop();
                }
                s
            }

        }
    }
}

impl Bot {

    async fn run_one_background_loop(&self) -> Result<(), BotError> {
        let events_to_fire = self.dependency.event_repository.get_events_to_fire(Utc::now()).await?;
        let event_ids = events_to_fire.iter().map(|e| e.event_id).collect::<Vec<_>>();
        let reply_markup = InlineKeyboardMarkup {
            inline_keyboard: vec![]
        };
        for event in events_to_fire {
            info!("{:?}", event);
            self.dependency.tg.send_message(event.user_id, event.text, Some(reply_markup.clone())).await?;
        }
        self.dependency.event_repository.delete_events(event_ids).await?;

        Ok(())
    }

    async fn run_background(&self) {
        info!("Background loop started");
        loop {
            match self.run_one_background_loop().await {
                Ok(_) => (),
                Err(err) => {
                    error!("Error in background loop: {}", err);
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    pub fn run_background_task(self) -> JoinHandle<()> {
        tokio::spawn(async move { self.run_background().await })
    }

    pub async fn run(&self) -> Result<(), BotError> {
        let mut last_offset = 0_u64;
        let mut state = FnvHashMap::default();
        let (state_sender, mut state_receiver) = tokio::sync::mpsc::unbounded_channel();
        info!("Bot is started");
        loop {
            let updates = self.dependency.tg.get_updates(last_offset).await;
            match updates {
                Ok(updates) => {
                    for update in updates {
                        last_offset = update.update_id + 1;

                        if let Some(chat_id) = update.get_chat_id() {
                            if !self.dependency.user_repository.is_chat_id_valid(chat_id) {
                                continue;
                            }

                            info!("{:?}", update);

                            let bot_handler = BotHandler {
                                bot: self.dependency.clone(),
                                state: state.get(&chat_id).cloned().unwrap_or(State::Idle),
                                state_channel: state_sender.clone()
                            };
                            let _ = tokio::spawn(async move {
                                let err = bot_handler.handle_update(update).await;
                                if let Err(err) = err {
                                    info!("Error in update handler: {}", err);
                                }
                            });
                        }
                    }
                },
                Err(err) => {
                    info!("Error: {}", err);
                }
            }

            while let Ok((chat_id, new_state)) = state_receiver.try_recv() {
                state.insert(chat_id, new_state);
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}