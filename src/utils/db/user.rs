use actix_web::web;
use diesel::{dsl, ExpressionMethods, QueryDsl, RunQueryDsl};
use uuid::Uuid;

use crate::definitions::*;
use crate::handlers::request_io::{InputEditUser, InputUser};
use crate::models::user::{NewUser, User};
use crate::schema::users as user_fields;
use crate::schema::users::dsl::users;
use crate::utils::password_hasher;

pub fn get_user_by_id(
    db_connection: &DbConnection,
    user_id: Uuid,
) -> Result<User, diesel::result::Error> {
    users.find(user_id).first::<User>(db_connection)
}

pub fn get_user_by_email(
    db_connection: &DbConnection,
    user_email: &str,
) -> Result<User, diesel::result::Error> {
    users
        .filter(user_fields::email.eq(user_email.to_lowercase()))
        .first::<User>(db_connection)
}

pub fn create_user(
    db_connection: &DbConnection,
    user_data: &web::Json<InputUser>,
) -> Result<User, diesel::result::Error> {
    let hashed_password = password_hasher::hash_password(&user_data.password);
    let current_time = chrono::Utc::now().naive_utc();

    let new_user = NewUser {
        id: Uuid::new_v4(),
        is_active: true,
        is_premium: false,
        premium_expiration: Option::None,
        email: &user_data.email.to_lowercase(),
        password_hash: &hashed_password,
        first_name: &user_data.first_name,
        last_name: &user_data.last_name,
        date_of_birth: user_data.date_of_birth,
        modified_timestamp: current_time,
        created_timestamp: current_time,
        currency: &user_data.currency,
    };

    dsl::insert_into(users)
        .values(&new_user)
        .get_result::<User>(db_connection)
}

pub fn edit_user(
    db_connection: &DbConnection,
    user_id: Uuid,
    edited_user_data: &web::Json<InputEditUser>,
) -> Result<(), diesel::result::Error> {
    match dsl::update(users.filter(user_fields::id.eq(user_id)))
        .set((
            user_fields::first_name.eq(&edited_user_data.first_name),
            user_fields::last_name.eq(&edited_user_data.last_name),
            user_fields::date_of_birth.eq(&edited_user_data.date_of_birth),
            user_fields::currency.eq(&edited_user_data.currency),
        ))
        .execute(db_connection)
    {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn change_password(
    db_connection: &DbConnection,
    user_id: Uuid,
    new_password: &str,
) -> Result<(), diesel::result::Error> {
    let hashed_password = password_hasher::hash_password(new_password);

    match dsl::update(users.filter(user_fields::id.eq(user_id)))
        .set(user_fields::password_hash.eq(hashed_password))
        .execute(db_connection)
    {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::NaiveDate;
    use rand::prelude::*;

    use crate::env;

    #[actix_rt::test]
    async fn test_create_user() {
        let db_thread_pool = &*env::testing::DB_THREAD_POOL;
        let db_connection = db_thread_pool.get().unwrap();

        const PASSWORD: &str = "X$KC3%s&L91m!bVA*@Iu";

        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let new_user = InputUser {
            email: format!("test_user{}@test.com", &user_number),
            password: PASSWORD.to_string(),
            first_name: format!("Test-{}", &user_number),
            last_name: format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: String::from("USD"),
        };

        let new_user_json = web::Json(new_user.clone());
        create_user(&db_connection, &new_user_json).unwrap();

        let created_user = users
            .filter(user_fields::email.eq(&new_user.email.to_lowercase()))
            .first::<User>(&db_connection)
            .unwrap();

        assert_eq!(&new_user.email, &created_user.email);
        assert_ne!(&new_user.password, &created_user.password_hash);
        assert_eq!(&new_user.first_name, &created_user.first_name);
        assert_eq!(&new_user.last_name, &created_user.last_name);
        assert_eq!(&new_user.date_of_birth, &created_user.date_of_birth);
        assert_eq!(&new_user.currency, &created_user.currency);
    }

    #[actix_rt::test]
    async fn test_get_user_by_email() {
        let db_thread_pool = &*env::testing::DB_THREAD_POOL;
        let db_connection = db_thread_pool.get().unwrap();

        const PASSWORD: &str = "Uo^Z56o%f#@8Ub#I9D&f";

        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let user_email = format!("test_user{}@test.com", &user_number);
        let new_user = InputUser {
            email: user_email.clone(),
            password: PASSWORD.to_string(),
            first_name: format!("Test-{}", &user_number),
            last_name: format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: String::from("USD"),
        };

        let new_user_json = web::Json(new_user.clone());
        create_user(&db_connection, &new_user_json).unwrap();

        let created_user = get_user_by_email(&db_connection, &user_email).unwrap();

        assert_eq!(&new_user.email, &created_user.email);
        assert_ne!(&new_user.password, &created_user.password_hash);
        assert_eq!(&new_user.first_name, &created_user.first_name);
        assert_eq!(&new_user.last_name, &created_user.last_name);
        assert_eq!(&new_user.date_of_birth, &created_user.date_of_birth);
        assert_eq!(&new_user.currency, &created_user.currency);
    }

    #[actix_rt::test]
    async fn test_get_user_by_id() {
        let db_thread_pool = &*env::testing::DB_THREAD_POOL;
        let db_connection = db_thread_pool.get().unwrap();

        const PASSWORD: &str = "Uo^Z56o%f#@8Ub#I9D&f";

        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let new_user = InputUser {
            email: format!("test_user{}@test.com", &user_number),
            password: PASSWORD.to_string(),
            first_name: format!("Test-{}", &user_number),
            last_name: format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: String::from("USD"),
        };

        let new_user_json = web::Json(new_user.clone());
        let user_id = create_user(&db_connection, &new_user_json).unwrap().id;

        let created_user = get_user_by_id(&db_connection, user_id).unwrap();

        assert_eq!(&new_user.email, &created_user.email);
        assert_ne!(&new_user.password, &created_user.password_hash);
        assert_eq!(&new_user.first_name, &created_user.first_name);
        assert_eq!(&new_user.last_name, &created_user.last_name);
        assert_eq!(&new_user.date_of_birth, &created_user.date_of_birth);
        assert_eq!(&new_user.currency, &created_user.currency);
    }

    #[actix_rt::test]
    async fn test_edit_user_one_field() {
        let db_thread_pool = &*env::testing::DB_THREAD_POOL;
        let db_connection = db_thread_pool.get().unwrap();

        const PASSWORD: &str = "C4R1pUr2E2fG5qKPT&&s";

        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let new_user = InputUser {
            email: format!("test_user{}@test.com", &user_number),
            password: PASSWORD.to_string(),
            first_name: format!("Test-{}", &user_number),
            last_name: format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: String::from("USD"),
        };

        let new_user_json = web::Json(new_user);
        let user_before = create_user(&db_connection, &new_user_json).unwrap();

        let user_edits = InputEditUser {
            first_name: String::from("Edited Name"),
            last_name: user_before.last_name.clone(),
            date_of_birth: user_before.date_of_birth.clone(),
            currency: user_before.currency.clone(),
        };

        let user_edits_json = web::Json(user_edits.clone());
        edit_user(&db_connection, user_before.id, &user_edits_json).unwrap();

        let user_after = get_user_by_id(&db_connection, user_before.id).unwrap();

        assert_eq!(&user_after.email, &user_before.email);
        assert_eq!(&user_after.last_name, &user_before.last_name);
        assert_eq!(&user_after.date_of_birth, &user_before.date_of_birth);
        assert_eq!(&user_after.currency, &user_before.currency);

        assert_eq!(&user_after.password_hash, &user_before.password_hash);

        assert_eq!(&user_after.first_name, &user_edits.first_name);
    }

    #[actix_rt::test]
    async fn test_edit_user_all_fields() {
        let db_thread_pool = &*env::testing::DB_THREAD_POOL;
        let db_connection = db_thread_pool.get().unwrap();

        const PASSWORD: &str = "C4R1pUr2E2fG5qKPT&&s";

        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let new_user = InputUser {
            email: format!("test_user{}@test.com", &user_number),
            password: PASSWORD.to_string(),
            first_name: format!("Test-{}", &user_number),
            last_name: format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: String::from("USD"),
        };

        let new_user_json = web::Json(new_user.clone());
        let user_before = create_user(&db_connection, &new_user_json).unwrap();

        let user_edits = InputEditUser {
            first_name: String::from("Edited"),
            last_name: String::from("Name"),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1940..=1949),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: String::from("DOP"),
        };

        let user_edits_json = web::Json(user_edits.clone());
        edit_user(&db_connection, user_before.id, &user_edits_json).unwrap();

        let user_after = get_user_by_id(&db_connection, user_before.id).unwrap();

        assert_eq!(&user_after.password_hash, &user_before.password_hash);

        assert_eq!(&user_after.email, &new_user.email);
        assert_eq!(&user_after.first_name, &user_edits.first_name);
        assert_eq!(&user_after.last_name, &user_edits.last_name);
        assert_eq!(&user_after.date_of_birth, &user_edits.date_of_birth);
        assert_eq!(&user_after.currency, &user_edits.currency);
    }

    #[actix_rt::test]
    async fn test_change_password() {
        let db_thread_pool = &*env::testing::DB_THREAD_POOL;
        let db_connection = db_thread_pool.get().unwrap();

        const ORIGINAL_PASSWORD: &str = "Eq&6T@Vyz54O%DoX$";
        const UPDATED_PASSWORD: &str = "P*%OaTMaMl^Uzft^$82Qn";

        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let new_user = InputUser {
            email: format!("test_user{}@test.com", &user_number),
            password: ORIGINAL_PASSWORD.to_string(),
            first_name: format!("Test-{}", &user_number),
            last_name: format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: String::from("USD"),
        };

        let new_user_json = web::Json(new_user);
        let user_id = create_user(&db_connection, &new_user_json).unwrap().id;

        let original_password_saved_hash = users
            .find(user_id)
            .select(user_fields::password_hash)
            .get_result::<String>(&db_connection)
            .unwrap();

        assert!(password_hasher::verify_hash(
            ORIGINAL_PASSWORD,
            &original_password_saved_hash
        ));

        change_password(&db_connection, user_id, UPDATED_PASSWORD).unwrap();

        let updated_password_saved_hash = users
            .find(user_id)
            .select(user_fields::password_hash)
            .get_result::<String>(&db_connection)
            .unwrap();

        assert_ne!(original_password_saved_hash, updated_password_saved_hash);
        assert!(password_hasher::verify_hash(
            UPDATED_PASSWORD,
            &updated_password_saved_hash
        ));
    }
}
