use reqwest::Url;
use crate::errors::BotError;
use crate::models::{EditMessage, GetUpdatesResponse, InlineKeyboardMarkup, SendMessage, Update};

#[derive(Clone)]
pub struct Tg {
    client: reqwest::Client,
    key: String
}

impl Tg {
    pub fn new(key: String) -> Tg {
        let client = reqwest::Client::new();
        Tg { client, key }
    }

    pub async fn get_updates(&self, offset: u64) -> Result<Vec<Update>, BotError> {
        let url = format!("https://api.telegram.org/bot{}/getUpdates?offset={}", self.key, offset);
        let updates: GetUpdatesResponse = self.client.get(&url)
            .send()
            .await?
            .json()
            .await?;
        Ok(updates.result)
    }

    pub async fn answer_callback_query(&self, callback_query_id: String, text: Option<String>) -> Result<(), BotError> {
        let base = format!("https://api.telegram.org/bot{}/answerCallbackQuery", self.key);
        let mut url: Url = Url::parse(&base)?;
        {
            let mut params = url.query_pairs_mut();
            params.append_pair("callback_query_id", &callback_query_id);
            if let Some(text) = text {
                params.append_pair("text", &text);
            }
        }
        self.client.get(url).send().await?;
        Ok(())
    }

    pub async fn send_message(&self, chat_id: u64, text: String, reply_markup: Option<InlineKeyboardMarkup>) -> Result<(), BotError> {
        // send post request with SendMessage in json in body
        let base = format!("https://api.telegram.org/bot{}/sendMessage", self.key);
        let url: Url = Url::parse(&base)?;
        let send_message = SendMessage {
            chat_id,
            text,
            reply_markup
        };
        self.client.post(url).json(&send_message).send().await?;
        Ok(())
    }

    pub async fn edit_message_text(&self, chat_id: u64, message_id: u64, text: String, reply_markup: Option<InlineKeyboardMarkup>) -> Result<(), BotError> {
        // send post request with SendMessage in json in body
        let base = format!("https://api.telegram.org/bot{}/editMessageText", self.key);
        let url: Url = Url::parse(&base)?;
        let send_message = EditMessage {
            chat_id,
            message_id,
            text,
            reply_markup
        };
        self.client.post(url).json(&send_message).send().await?;
        Ok(())
    }

    pub async fn delete_message(&self, chat_id: u64, message_id: u64) -> Result<(), BotError> {
        let base = format!("https://api.telegram.org/bot{}/deleteMessage", self.key);
        let mut url: Url = Url::parse(&base)?;
        {
            let mut params = url.query_pairs_mut();
            params.append_pair("chat_id", &chat_id.to_string());
            params.append_pair("message_id", &message_id.to_string());
        }
        self.client.get(url).send().await?;
        Ok(())
    }
}