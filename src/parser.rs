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
struct OpenAIRequest {
    model: String,
    prompt: String,
    temperature: f64,
    max_tokens: u16,
    top_p: f64,
    frequency_penalty: f64,
    presence_penalty: f64,
    stop: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ModelChoice {
    pub text: String,
    pub index: u16,
}

#[derive(Debug, Deserialize)]
pub struct ModelResponse {
    pub choices: Vec<ModelChoice>,
}

impl OpenAIParser {
    pub fn new(api_key: String) -> OpenAIParser {
        let client = reqwest::Client::new();
        OpenAIParser { api_key, client }
    }

    fn create_prompt(current_date: DateTime<Utc>, text: &str) -> String {
        let current_date_as_naive = current_date.naive_utc();
        let current_date = chrono_tz::Israel.from_utc_datetime(&current_date_as_naive);
        // format should be like 21.07.2022 22:37:01, thursday
        let formatted_date = current_date.format("%d.%m.%Y %H:%M:%S, %A");
        format!("/* Examples of how notifications should be parsed into two possible types:
Type 1: absolute date and time of format {{\"kind\": \"abs\", \"text\": \"string\", \"times\": [\"22.07.2022 03:37:01\"]}}
Type 2: relative to current date and time of format {{\"kind\": \"rel\", \"text\": \"string\", \"week\": 0, \"days\": [5], \"time\": \"12:00\"}}
*/
const current_time = \"21.07.2022 22:37:01, Thursday\";
const query = 'Remind me about \"собеседование\" in five hours';
const answer = {{\"kind\": \"abs\", \"text\": \"собеседование\", \"times\": [\"22.07.2022 03:37:01\"]}};
***
const current_time = \"21.07.2022 22:37:01, Thursday\";
const query = 'Remind me about \"собеседование\" next friday at 12:00';
const answer = {{\"kind\": \"rel\", \"text\": \"собеседование\", \"week\": 1, \"days\": [5], \"times\": [\"12:00\"]}};
***
const current_time = \"24.01.2023 14:00:00, Tuesday\";
const query = 'Напомни мне позвонить Алексу в субботу днём';
const answer = {{\"kind\": \"rel\", \"text\": \"позвонить Алексу\", \"week\": 0, \"days\": [6], \"times\": [\"12:00\"]}};
***
const current_time = \"25.02.2023 18:00:00, Tuesday\";
const query = 'Через два и три часа напомни мне проверить плиту';
const answer = {{\"kind\": \"abs\", \"text\": \"проверить плиту\", \"times\": [\"25.02.2023 20:00:00\", \"25.02.2023 21:00:00\"]}};
***
const current_time = \"{formatted_date}\";
const query = '{text}';
const answer = {{\"kind\": \"")
    }

    pub async fn parse(&self, current_date: DateTime<Utc>, text: &str) -> Result<Notification, BotError> {
        let request = OpenAIRequest {
            model: "code-davinci-002".to_string(),
            prompt: Self::create_prompt(current_date, text),
            temperature: 0.0,
            max_tokens: 256,
            top_p: 1.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            stop: vec!["***".to_string(), "\n".to_string()],
        };

        // like curl above
        let model = self.client.post("https://api.openai.com/v1/completions")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send().await?
            .json::<ModelResponse>().await?;

        Self::parse_response(model)
    }
    
    fn parse_response(model_response: ModelResponse) -> Result<Notification, BotError> {
        let choice = model_response.choices.first().ok_or(BotError::NoCompletionGiven)?;
        let json_string = format!("{{\"kind\": \"{}", choice.text.trim_end_matches(";"));

        info!("\"{}\"", json_string);

        let notification: Notification = serde_json::from_str(&json_string)?;

        Ok(notification)
    }
}

#[cfg(test)]
mod tests {
    use arrayvec::ArrayVec;
    use chrono::{Utc, DateTime};

    use crate::models::{Notification, FormattedTime};

    use super::{OpenAIParser, ModelResponse};

    #[test]
    fn should_create_prompt_as_expected() {
        let current_date = DateTime::parse_from_rfc3339("2023-01-26T14:40:00+02:00").unwrap();
        let current_date_in_utc = current_date.with_timezone(&Utc);
        let text = "Завтра в 12 и 15 часов напомни проверить почту";
        let prompt = OpenAIParser::create_prompt(current_date_in_utc, text);

        // read prompt from assets/example_prompt.txt
        let expected_prompt = std::fs::read_to_string("assets/example_prompt.txt").unwrap().replace("\r", "");

        assert_eq!(prompt, expected_prompt);
    }

    #[test]
    fn should_parse_absolute_completion_as_expected() {
        let completion = ModelResponse {
            choices: vec![
                super::ModelChoice { 
                    text: "abs\", \"text\": \"проверить почту\", \"times\": [\"27.01.2023 12:00:00\", \"27.01.2023 15:00:00\"]};".to_owned(), 
                    index: 0,
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
        let completion = ModelResponse {
            choices: vec![
                super::ModelChoice { 
                    text: "rel\", \"text\": \"проверить почту\", \"week\": 0, \"days\": [5], \"times\": [\"12:00\", \"15:00\"]};".to_owned(), 
                    index: 0,
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