use chrono::NaiveDateTime;
use diesel::{Insertable, Queryable};
use serde::{Deserialize, Serialize};

use crate::models::budget::Budget;
use crate::schema::categories;

#[derive(Clone, Debug, Serialize, Deserialize, Associations, Identifiable, Queryable)]
#[belongs_to(Budget, foreign_key = "budget_id")]
#[table_name = "categories"]
pub struct Category {
    pub pk: i32,
    pub budget_id: uuid::Uuid,
    pub is_deleted: bool,
    pub id: i16,
    pub name: String,
    pub limit_cents: i64,
    pub color: String,
    pub modified_timestamp: NaiveDateTime,
    pub created_timestamp: NaiveDateTime,
}

#[derive(Clone, Debug, Insertable)]
#[table_name = "categories"]
pub struct NewCategory<'a> {
    pub budget_id: uuid::Uuid,
    pub is_deleted: bool,
    pub id: i16,
    pub name: &'a str,
    pub limit_cents: i64,
    pub color: &'a str,
    pub modified_timestamp: NaiveDateTime,
    pub created_timestamp: NaiveDateTime,
}
