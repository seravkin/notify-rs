use chrono::{Datelike, DateTime, Timelike, Utc};
use deadpool_sqlite::Runtime;
use fnv::FnvHashSet;
use rusqlite::ToSql;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ValueRef};
use crate::errors::BotError;
use crate::models::{EventToFire, StoredNotification};


#[derive(Clone, Debug)]
pub struct UserRepository {
    users: FnvHashSet<u64>
}

impl UserRepository {
    pub fn new(users: impl Iterator<Item = u64>) -> Self {
        Self {
            users: FnvHashSet::from_iter(users)
        }
    }

    pub fn is_chat_id_valid(&self, chat_id: u64) -> bool {
        self.users.contains(&chat_id)
    }
}

#[derive(Clone, Debug)]
pub struct EventRepository {
    pool: deadpool_sqlite::Pool,
}

#[derive(Debug)]
pub enum Kind {
    Absolute,
    Recurrent,
}

impl FromSql for Kind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value {
            ValueRef::Text(text) => match text {
                b"absolute" => Ok(Kind::Absolute),
                b"recurrent" => Ok(Kind::Recurrent),
                _ => Err(FromSqlError::InvalidType)
            },
            _ => Err(FromSqlError::InvalidType)
        }
    }
}

#[derive(Debug)]
pub struct Event {
    pub id: u64,
    pub kind: Kind,
    pub user_id: u64,
    pub text: String,
    pub time: Option<DateTime<Utc>>,
    pub day: Option<u8>,
    pub hour: Option<u8>,
    pub minute: Option<u8>,
    pub is_deleted: bool
}


impl EventRepository {
    pub async fn new(connection_string: &str) -> Result<EventRepository, BotError> {
        let cfg = deadpool_sqlite::Config::new(connection_string);
        let pool = cfg.create_pool(Runtime::Tokio1)?;
        let connection = pool.get().await?;
        connection.interact(|connection| {
            // in sqlite syntax create table event if not exists
            // create also indexes on (user_id, is_deleted)
            let sql = "create table if not exists event (
                id integer primary key autoincrement,
                kind text not null,
                user_id integer not null,
                event_text text not null,
                event_time datetime,
                day integer,
                hour integer,
                minute integer,
                is_deleted integer
            );

            create index if not exists event_user_id_is_deleted on event (user_id, is_deleted);
            create index if not exists event_is_deleted on event (is_deleted);";
            connection.execute(sql, ())
        }).await??;
        Ok(EventRepository { pool })
    }

    pub async fn insert_event(&self, user_id: u64, text: String, stored_notification: Vec<StoredNotification>) -> Result<Vec<u64>, BotError> {
        let ids = self.pool.get().await?.interact(move |connection| {
            let tx = connection.transaction()?;
            let mut ids = vec![];
            {
                let mut stmt = tx.prepare_cached("insert into event (kind, user_id, event_text, event_time, day, hour, minute, is_deleted) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);")?;

                for notification in stored_notification {
                    match notification {
                        StoredNotification::Absolute { time, .. } => {
                            let u: Option<u8> = None;
                            let u: &dyn ToSql = &u;
                            stmt.execute(&[&"absolute" as &dyn ToSql, &user_id, &text, &Some(time), u, u, u, &0 as &dyn ToSql])?;
                            // get last inserted rowid
                            ids.push(tx.last_insert_rowid() as u64);
                        }
                        StoredNotification::Recurrent { hours, minutes, days, .. } => {
                            if let Some(days) = days {
                                for day in days.iter() {
                                    let none: Option<DateTime<Utc>> = None;
                                    stmt.execute(&[&"recurrent" as &dyn ToSql, &user_id, &text, &none, &Some(*day), &Some(hours), &Some(minutes), &0 as &dyn ToSql])?;
                                    ids.push(tx.last_insert_rowid() as u64);
                                }
                            }
                        }
                    };
                }
            }
            tx.commit().map(|_| ids)
        }).await??;
        Ok(ids)
    }

    pub async fn delete_events(&self, event_ids: Vec<u64>) -> Result<(), BotError> {
        self.pool.get().await?.interact(move |connection| {
            rusqlite::vtab::array::load_module(&connection)?;
            let array = rusqlite::vtab::array::Array::new(
                event_ids.iter()
                    .map(|x| rusqlite::types::Value::Integer(*x as i64))
                    .collect()
            );
            connection.execute("update event set is_deleted = 1 where id in rarray(?);", [array])
        }).await??;
        Ok(())
    }

    pub async fn get_events_to_fire(&self, current_time: DateTime<Utc>) -> Result<Vec<EventToFire>, BotError> {
        // select only rows which has kind absolute and time is after current time or
        // kind recurrent and current day is equal to day and hour + minute is after current time
        let events = self.pool.get().await?
            .interact(move |connection| {
                let current_day = current_time.weekday().num_days_from_monday() + 1;
                let minutes = current_time.hour() * 60 + current_time.minute();
                let mut stmt = connection
                    .prepare("select id, user_id, event_text from event where \
                is_deleted = 0 and (
                kind = 'absolute' and event_time < ? or \
                kind = 'recurrent' and day = ? and hour * 60 + minute < ?)")?;

                let result = stmt.query_map(&[&current_time as &dyn ToSql, &current_day, &minutes], |row| {
                    let event_id: u64 = row.get(0)?;
                    let user_id: u64 = row.get(1)?;
                    let text: String = row.get(2)?;
                    Ok(EventToFire {
                        event_id,
                        user_id,
                        text
                    })
                })?.collect::<Result<Vec<_>, _>>();
                result
            }).await??;
        Ok(events)
    }
}