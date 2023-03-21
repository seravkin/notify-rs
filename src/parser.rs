use chrono::{DateTime, TimeZone, Utc};
use log::info;
use serde::{Deserialize, Serialize};
use crate::errors::BotError;
use crate::models::Notification;

#[derive(Clone)]
pub struct OpenAIParser {
    pub api_key: String,
    pub client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct OpenAIChatRequest {
    model: String,
    messages: Vec<Message>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message
}

impl OpenAIParser {
    pub fn new(api_key: String) -> OpenAIParser {
        let client = reqwest::Client::new();
        OpenAIParser { api_key, client }
    }

    const SYSTEM_PROMPT: &'static str = "You are an assistant tasked with converting user queries into json formatted notifications. You shouldn't comment on the query, just output the json. 

Examples of how notifications should be parsed into two possible types:
Type 1: absolute date and time of format {\"kind\": \"absolute\", \"text\": \"string\", \"times\": [\"22.07.2022 03:37:01\"]}
Type 2: relative to current date and time of format {\"kind\": \"relative\", \"text\": \"string\", \"week\": 0, \"days\": [5], \"time\": \"12:00\"}

Examples of queries:

Current time is \"21.07.2022 22:37:01, Thursday\"
Remind me about \"собеседование\" in five hours

Answer: {\"kind\": \"absolute\", \"text\": \"собеседование\", \"times\": [\"22.07.2022 03:37:01\"]}

Current time is \"21.07.2022 22:37:01, Thursday\"
Remind me about \"собеседование\" next friday at 12:00

Answer: {\"kind\": \"relative\", \"text\": \"собеседование\", \"week\": 1, \"days\": [5], \"times\": [\"12:00\"]}

Current time is \"24.01.2023 14:00:00, Tuesday\"
Напомни мне позвонить Алексу в субботу днём;

Answer: {\"kind\": \"relative\", \"text\": \"позвонить Алексу\", \"week\": 0, \"days\": [6], \"times\": [\"12:00\"]}

Current time is \"25.02.2023 18:00:00, Tuesday\"
'Через два и три часа напомни мне проверить плиту'

Answer: {\"kind\": \"absolute\", \"text\": \"проверить плиту\", \"times\": [\"25.02.2023 20:00:00\", \"25.02.2023 21:00:00\"]}";

    fn create_prompt(current_date: DateTime<Utc>, text: &str) -> (String, String) {
        let current_date_as_naive = current_date.naive_utc();
        let current_date = chrono_tz::Israel.from_utc_datetime(&current_date_as_naive);
        // format should be like 21.07.2022 22:37:01, thursday
        let formatted_date = current_date.format("%d.%m.%Y %H:%M:%S, %A");

        (Self::SYSTEM_PROMPT.to_owned(), format!("Current time is \"{}\"\n{}\n", formatted_date, text))
    }

    pub async fn parse(&self, current_date: DateTime<Utc>, text: &str) -> Result<Notification, BotError> {
        let (system_message, user_message) = Self::create_prompt(current_date, text);

        let request = OpenAIChatRequest {
            model: "gpt-3.5-turbo".to_owned(),
            messages: vec![
                Message {
                    role: "system".to_owned(),
                    content: system_message,
                },
                Message {
                    role: "user".to_owned(),
                    content: user_message,
                },
            ],
        };

        // like curl above
        let model = self.client.post("https://api.openai.com/v1/chat/completions")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send().await?
            .json::<OpenAIChatResponse>().await?;

        Self::parse_response(model)
    }
    
    fn parse_response(model_response: OpenAIChatResponse) -> Result<Notification, BotError> {
        let choice = model_response.choices.first().ok_or(BotError::NoCompletionGiven)?;

        info!("\"{}\"", choice.message.content);

        let notification: Notification = serde_json::from_str(&choice.message.content)?;

        Ok(notification)
    }
}

#[cfg(test)]
mod tests {
    use arrayvec::ArrayVec;
    use chrono::{Utc, DateTime};

    use crate::models::{Notification, FormattedTime};

    use super::{OpenAIParser, OpenAIChatResponse};

    #[test]
    fn should_create_prompt_as_expected() {
        let current_date = DateTime::parse_from_rfc3339("2023-01-26T14:40:00+02:00").unwrap();
        let current_date_in_utc = current_date.with_timezone(&Utc);
        let text = "Завтра в 12 и 15 часов напомни проверить почту";
        let (system_prompt, user_prompt) = OpenAIParser::create_prompt(current_date_in_utc, text);

        // read prompt from assets/example_prompt.txt
        let expected_prompt = std::fs::read_to_string("assets/example_prompt.txt").unwrap().replace("\r", "");

        assert_eq!(system_prompt, expected_prompt);

        assert_eq!("Current time is \"26.01.2023 14:40:00, Thursday\"\nЗавтра в 12 и 15 часов напомни проверить почту\n", user_prompt)
    }

    #[test]
    fn should_parse_absolute_completion_as_expected() {
        let completion = OpenAIChatResponse {
            choices: vec![
                super::Choice { 
                    message: super::Message { 
                        content: "{\"kind\": \"absolute\", \"text\": \"проверить почту\", \"times\": [\"27.01.2023 12:00:00\", \"27.01.2023 15:00:00\"]}".to_owned(), 
                        role: "assistant".to_owned(), 
                    },
                }
            ]
        };

        let notification = OpenAIParser::parse_response(completion).unwrap();

        match notification {
            Notification::Absolute { text, times } => {
                assert_eq!(text, "проверить почту");
                let expected_time_one = DateTime::parse_from_rfc3339("2023-01-27T12:00:00+02:00").unwrap();
                let expected_time_two = DateTime::parse_from_rfc3339("2023-01-27T15:00:00+02:00").unwrap();
                let formatted_time_array = vec![FormattedTime { time: expected_time_one.into() }, FormattedTime { time: expected_time_two.into() }];
                assert_eq!(times, formatted_time_array);
            },
            _ => panic!("Notification should be absolute"),
        }
    }

    #[test]
    fn should_parse_relative_completion_as_expected() {
        let completion = OpenAIChatResponse {
            choices: vec![
                super::Choice { 
                    message: super::Message { 
                        role: "assistant".to_owned(), 
                        content: "{\"kind\": \"relative\", \"text\": \"проверить почту\", \"week\": 0, \"days\": [5], \"times\": [\"12:00\", \"15:00\"]}".to_owned(), 
                    },
                }
            ]
        };

        let notification = OpenAIParser::parse_response(completion).unwrap();

        match notification {
            Notification::Relative { text, week, days, times } => {
                assert_eq!(text, "проверить почту");
                assert_eq!(week, 0);
                assert_eq!(days, ArrayVec::from_iter(std::iter::once(5)));
                assert_eq!(times[0].hours, 12);
                assert_eq!(times[0].minutes, 0);
                assert_eq!(times[1].hours, 15);
                assert_eq!(times[1].minutes, 0);
            },
            _ => panic!("Notification should be relative"),
        }
    }
}