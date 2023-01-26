use std::str::FromStr;
use arrayvec::ArrayVec;
use chrono::{Datelike, DateTime, Duration, Timelike, TimeZone, Utc};
use envconfig::Envconfig;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde::de::Error;
use crate::errors::BotError;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Chat { pub id: u64, }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub message_id: u64,
    pub date: u64,
    pub chat: Chat,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Update {
    pub update_id: u64,
    pub message: Option<Message>,
    pub edited_message: Option<Message>,
    pub callback_query: Option<CallbackQuery>
}

impl Update {
    pub fn get_chat_id(&self) -> Option<u64> {
        self.message.as_ref().map(|m| m.chat.id)
            .or(self.edited_message.as_ref().map(|m| m.chat.id))
            .or(self.callback_query.as_ref().map(|m| m.from.id))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineKeyboardButton {
    pub text: String,
    pub callback_data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessage {
    pub chat_id: u64,
    pub text: String,
    pub reply_markup: Option<InlineKeyboardMarkup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditMessage {
    pub chat_id: u64,
    pub message_id: u64,
    pub text: String,
    pub reply_markup: Option<InlineKeyboardMarkup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: u64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    pub chat: Option<Chat>,
    pub message: Option<Message>,
    pub data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetUpdatesResponse {
    pub result: Vec<Update>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageResponse {
    pub result: Message,
}

#[derive(Debug, Clone)]
pub struct CommaSeparatedIds(Vec<u64>);

impl CommaSeparatedIds {
    pub fn iter(&self) -> impl Iterator<Item = &u64> {
        self.0.iter()
    }
}

impl FromStr for CommaSeparatedIds {
    type Err = BotError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // if empty return error
        if s.is_empty() {
            return Err(BotError::EnvIds);
        }

        let ids: Vec<u64> = s.split(',')
            .map(|s| s.parse::<u64>())
            .collect::<Result<_, _>>()?;

        Ok(CommaSeparatedIds(ids))
    }
}

#[derive(Debug, Clone, Envconfig)]
pub struct Env {
    #[envconfig(from = "TG_KEY")]
    pub bot_token: String,
    #[envconfig(from = "OAI_TOKEN")]
    pub openai_token: String,
    #[envconfig(from = "TG_USERS")]
    pub user_ids: CommaSeparatedIds,
    #[envconfig(from = "CONN_STRING")]
    pub connection_string: String
}

#[derive(Debug, Clone)]
pub struct Time {
    pub hours: u8,
    pub minutes: u8,
}

impl Serialize for Time {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{:02}:{:02}", self.hours, self.minutes))
    }
}

impl<'de> Deserialize<'de> for Time {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let s = <&str>::deserialize(deserializer)?;
        let mut parts = s.split(':');
        let hours = parts
            .next()
            .ok_or(D::Error::custom("no hours found"))?
            .parse::<u8>()
            .map_err(|x| D::Error::custom(x))?;
        let minutes = parts
            .next()
            .ok_or(D::Error::custom("no minutes found"))?
            .parse::<u8>()
            .map_err(|x| D::Error::custom(x))?;
        Ok(Time { hours, minutes })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedTime {
    pub time: DateTime<Utc>
}

// should be formatted like 21.07.2022 15:00
impl Serialize for FormattedTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let naive = self.time.naive_utc();
        let israel_time = chrono_tz::Israel.from_utc_datetime(&naive);
        serializer.serialize_str(&format!("{}", israel_time.format("%d.%m.%Y %H:%M:%S")))
    }
}

impl <'de> Deserialize<'de> for FormattedTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let s = <&str>::deserialize(deserializer)?;
        // deserialize in "%d.%m.%Y %H:%M" or "%d.%m.%Y %H:%M" format
        let time = chrono_tz::Israel.datetime_from_str(s, "%d.%m.%Y %H:%M")
            .or_else(|_| chrono_tz::Israel.datetime_from_str(s, "%d.%m.%Y %H:%M:%S"))
            .map_err(|x| D::Error::custom(x))?;
        let time = time.naive_utc();
        let time = Utc.from_utc_datetime(&time);

        Ok(FormattedTime { time })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Notification {
    #[serde(rename = "abs")]
    Absolute {
        text: String,
        times: Vec<FormattedTime>
    },
    #[serde(rename = "rel")]
    Relative {
        text: String,
        week: u8,
        days: ArrayVec<u8, 7>,
        times: Vec<Time>
    },
    #[serde(rename = "rec")]
    Recurrent {
        text: String,
        days: Option<ArrayVec<u8, 7>>,
        times: Vec<Time>
    }
}

#[derive(Debug, Clone)]
pub enum StoredNotification {
    Absolute {
        time: DateTime<Utc>,
    },
    Recurrent {
        hours: u8,
        minutes: u8,
        days: Option<ArrayVec<u8, 7>>,
    }
}

impl Notification {
    pub fn get_text(&self) -> &str {
        match self {
            Notification::Absolute { text, .. } => text.as_str(),
            Notification::Relative { text, .. } => text.as_str(),
            Notification::Recurrent { text, .. } => text.as_str(),
        }
    }

    pub fn create_stored_notifications(&self, current_time: DateTime<Utc>) -> Vec<StoredNotification> {
        match self {
            Notification::Absolute { times, .. } =>
                times.iter()
                    .map(|time| StoredNotification::Absolute { time: time.time })
                    .collect(),
            Notification::Relative {  week, days, times, .. } => {
                let current_day_of_week = (current_time.weekday().num_days_from_monday() + 1) as u8;
                let has_any_day_in_past = days.iter().any(|day| *day <= current_day_of_week);
                let week = if *week == 0 && has_any_day_in_past { 1 } else { *week };
                let monday = current_time
                    - Duration::days((current_day_of_week - 1) as i64)
                    + Duration::weeks(week as i64);
                days.iter()
                    .map(|x| (monday + Duration::days((*x - 1) as i64)))
                    .flat_map(|x| times.iter().map(move |time| (x, time)))
                    .filter_map(|(x, time)| Some(StoredNotification::Absolute {
                        time: x.with_hour(time.hours as u32)?.with_minute(time.minutes as u32)?
                    }))
                    .collect()
            }
            Notification::Recurrent { days, times, .. } => {
                times
                    .iter()
                    .map(|x| StoredNotification::Recurrent {
                        hours: x.hours,
                        minutes: x.minutes,
                        days: days.clone()
                    })
                    .collect()
            }
        }
    }
}

#[derive(Debug)]
pub struct EventToFire {
    pub event_id: u64,
    pub user_id: u64,
    pub text: String,
}

#[cfg(test)]
mod tests {

    #[test]
    fn should_parse_notification_from_json() {
        let json = r#"
{"kind": "abs", "text": "testing the bot", "times": ["24.07.2022 16:33:39"]}
        "#;
        let notification: super::Notification = serde_json::from_str(json).unwrap();
        assert_eq!(notification.get_text(), "testing the bot");
    }
}