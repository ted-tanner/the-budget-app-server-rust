use diesel::{dsl, ExpressionMethods, QueryDsl, RunQueryDsl};
use hmac::{Hmac, Mac};
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::definitions::*;
use crate::env;
use crate::models::blacklisted_token::{BlacklistedToken, NewBlacklistedToken};
use crate::schema::blacklisted_tokens as blacklisted_token_fields;
use crate::schema::blacklisted_tokens::dsl::blacklisted_tokens;

// TODO: This module needs to be refactored for clarity and performace

#[derive(Debug)]
pub enum TokenError {
    DatabaseError(diesel::result::Error),
    InvalidTokenType(TokenTypeError),
    TokenInvalid,
    TokenBlacklisted,
    TokenExpired,
    SystemResourceAccessFailure,
    WrongTokenType,
}

impl std::error::Error for TokenError {}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenError::DatabaseError(e) => write!(f, "DatabaseError: {}", e),
            TokenError::InvalidTokenType(e) => write!(f, "InvalidTokenType: {}", e),
            _ => write!(f, "Error: {}", self),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum TokenType {
    Access,
    Refresh,
    SignIn,
}

#[derive(Debug)]
pub enum TokenTypeError {
    NoMatchForValue(u8),
}

impl std::error::Error for TokenTypeError {}

impl fmt::Display for TokenTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenTypeError::NoMatchForValue(v) => write!(f, "NoMatchForValue: {}", v),
        }
    }
}

impl std::convert::TryFrom<u8> for TokenType {
    type Error = TokenTypeError;

    fn try_from(value: u8) -> Result<Self, TokenTypeError> {
        match value {
            0 => Ok(TokenType::Access),
            1 => Ok(TokenType::Refresh),
            2 => Ok(TokenType::SignIn),
            v => Err(TokenTypeError::NoMatchForValue(v)),
        }
    }
}

impl std::convert::From<TokenType> for u8 {
    fn from(token_type: TokenType) -> Self {
        match token_type {
            TokenType::Access => 0,
            TokenType::Refresh => 1,
            TokenType::SignIn => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TokenParams<'a> {
    pub user_id: &'a Uuid,
    pub user_email: &'a str,
    pub user_currency: &'a str,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct TokenClaims {
    pub exp: u64,    // Expiration in time since UNIX epoch
    pub uid: Uuid,   // User ID
    pub eml: String, // User email address
    pub cur: String, // User currency
    pub typ: u8,     // Token type (Access=0, Refresh=1, SignIn=2)
    pub slt: u32,    // Random salt (makes it so two tokens generated in the same
                     //              second are different--useful for testing)
}

impl TokenClaims {
    pub fn create_token(&self, key: &[u8]) -> String {
        let mut claims_and_hash =
            serde_json::to_vec(self).expect("Failed to transform claims into JSON");

        let mut mac =
            Hmac::<Sha256>::new_from_slice(key).expect("Failed to generate hash from key");
        mac.update(&claims_and_hash);
        let hash = hex::encode(mac.finalize().into_bytes());

        claims_and_hash.push(124); // 124 is the ASCII value of the | character
        claims_and_hash.extend_from_slice(&hash.into_bytes());

        base64::encode_config(claims_and_hash, base64::URL_SAFE_NO_PAD)
    }

    pub fn from_token_with_validation(token: &str, key: &[u8]) -> Result<TokenClaims, TokenError> {
        let (claims, claims_json_str, hash) = TokenClaims::token_to_claims_and_hash(token)?;

        let time_since_epoch = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(t) => t,
            Err(_) => return Err(TokenError::SystemResourceAccessFailure),
        };

        if time_since_epoch.as_secs() >= claims.exp {
            return Err(TokenError::TokenExpired);
        }

        let mut mac =
            Hmac::<Sha256>::new_from_slice(key).expect("Failed to generate hash from key");
        mac.update(&claims_json_str.as_bytes());

        match mac.verify_slice(&hash) {
            Ok(_) => Ok(claims),
            Err(_) => Err(TokenError::TokenInvalid),
        }
    }

    pub fn from_token_without_validation(token: &str) -> Result<TokenClaims, TokenError> {
        Ok(TokenClaims::token_to_claims_and_hash(token)?.0)
    }

    fn token_to_claims_and_hash<'a>(
        token: &'a str,
    ) -> Result<(TokenClaims, String, Vec<u8>), TokenError> {
        let decoded_token = match base64::decode_config(token.as_bytes(), base64::URL_SAFE_NO_PAD) {
            Ok(t) => t,
            Err(_) => return Err(TokenError::TokenInvalid),
        };

        let token_str = String::from_utf8_lossy(&decoded_token);
        let mut split_token = token_str.split('|').peekable();

        let mut claims_json_str = String::with_capacity(256);
        let mut hash_str = String::with_capacity(92);
        while let Some(part) = split_token.next() {
            if split_token.peek().is_none() {
                hash_str.push_str(part);
            } else {
                claims_json_str.push_str(part);
            }
        }

        let claims = match serde_json::from_str::<TokenClaims>(&claims_json_str) {
            Ok(c) => c,
            Err(_) => return Err(TokenError::TokenInvalid),
        };

        let hash = match hex::decode(&hash_str) {
            Ok(h) => h,
            Err(_) => return Err(TokenError::TokenInvalid),
        };

        Ok((claims, claims_json_str, hash))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InputBlacklistedRefreshToken {
    pub token: String,
    pub token_expiration_epoch: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Token {
    token: String,
    token_type: TokenType,
}

impl Token {
    #[allow(dead_code)]
    fn is_access_token(&self) -> bool {
        matches!(self.token_type, TokenType::Access)
    }

    #[allow(dead_code)]
    fn is_refresh_token(&self) -> bool {
        matches!(self.token_type, TokenType::Refresh)
    }

    #[allow(dead_code)]
    fn is_signin_token(&self) -> bool {
        matches!(self.token_type, TokenType::SignIn)
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.token)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TokenPair {
    pub access_token: Token,
    pub refresh_token: Token,
}

#[inline]
pub fn generate_access_token(params: TokenParams) -> Result<Token, TokenError> {
    generate_token(params, TokenType::Access)
}

#[inline]
pub fn generate_refresh_token(params: TokenParams) -> Result<Token, TokenError> {
    generate_token(params, TokenType::Refresh)
}

#[inline]
pub fn generate_signin_token(params: TokenParams) -> Result<Token, TokenError> {
    generate_token(params, TokenType::SignIn)
}

#[inline]
pub fn generate_token_pair(params: TokenParams) -> Result<TokenPair, TokenError> {
    let access_token = generate_access_token(params.clone())?;
    let refresh_token = generate_refresh_token(params)?;

    Ok(TokenPair {
        access_token,
        refresh_token,
    })
}

fn generate_token(params: TokenParams, token_type: TokenType) -> Result<Token, TokenError> {
    let lifetime_sec = match token_type {
        TokenType::Access => env::CONF.lifetimes.access_token_lifetime_mins * 60,
        TokenType::Refresh => env::CONF.lifetimes.refresh_token_lifetime_days * 24 * 60 * 60,
        // Because of how the one-time passcodes expire, a future passcode is sent to the user.
        // The verification endpoint checks the current code and the next (future) code, meaning
        // a user's code will be valid for a maximum of OTP_LIFETIME_SECS * 2.
        TokenType::SignIn => env::CONF.lifetimes.otp_lifetime_mins * 60 * 2,
    };

    let time_since_epoch = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(t) => t,
        Err(_) => return Err(TokenError::SystemResourceAccessFailure),
    };

    let expiration = time_since_epoch.as_secs() + lifetime_sec;
    let salt = rand::thread_rng().gen_range(1..u32::MAX);

    let claims = TokenClaims {
        exp: expiration,
        uid: *params.user_id,
        eml: params.user_email.to_string(),
        cur: params.user_currency.to_string(),
        typ: token_type.into(),
        slt: salt,
    };

    let token = claims.create_token(env::CONF.keys.token_signing_key.as_bytes());

    Ok(Token { token, token_type })
}

#[inline]
pub fn validate_access_token(token: &str) -> Result<TokenClaims, TokenError> {
    validate_token(token, TokenType::Access)
}

#[inline]
pub fn validate_refresh_token(
    token: &str,
    db_connection: &DbConnection,
) -> Result<TokenClaims, TokenError> {
    if is_on_blacklist(token, db_connection)? {
        return Err(TokenError::TokenBlacklisted);
    }

    validate_token(token, TokenType::Refresh)
}

#[inline]
pub fn validate_signin_token(token: &str) -> Result<TokenClaims, TokenError> {
    validate_token(token, TokenType::SignIn)
}

fn validate_token(token: &str, token_type: TokenType) -> Result<TokenClaims, TokenError> {
    let decoded_token = TokenClaims::from_token_with_validation(
        token,
        env::CONF.keys.token_signing_key.as_bytes(),
    )?;

    let token_type_claim = match TokenType::try_from(decoded_token.typ) {
        Ok(t) => t,
        Err(e) => return Err(TokenError::InvalidTokenType(e)),
    };

    if std::mem::discriminant(&token_type_claim) != std::mem::discriminant(&token_type) {
        Err(TokenError::WrongTokenType)
    } else {
        Ok(decoded_token)
    }
}

pub fn blacklist_token(
    token: &str,
    db_connection: &DbConnection,
) -> Result<BlacklistedToken, TokenError> {
    let decoded_token = TokenClaims::from_token_without_validation(token)?;

    let user_id = decoded_token.uid;
    let expiration = decoded_token.exp;

    let blacklisted_token = NewBlacklistedToken {
        token,
        user_id,
        token_expiration_time: match i64::try_from(expiration) {
            Ok(exp) => exp,
            Err(_) => return Err(TokenError::TokenInvalid),
        },
    };

    match dsl::insert_into(blacklisted_tokens)
        .values(&blacklisted_token)
        .get_result::<BlacklistedToken>(db_connection)
    {
        Ok(t) => Ok(t),
        Err(e) => Err(TokenError::DatabaseError(e)),
    }
}

pub fn is_on_blacklist(token: &str, db_connection: &DbConnection) -> Result<bool, TokenError> {
    match blacklisted_tokens
        .filter(blacklisted_token_fields::token.eq(token))
        .limit(1)
        .get_result::<BlacklistedToken>(db_connection)
    {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::NaiveDate;

    use crate::models::user::NewUser;
    use crate::schema::users::dsl::users;

    #[actix_rt::test]
    async fn test_create_token() {
        let claims = TokenClaims {
            exp: 123456789,
            uid: uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap(),
            eml: format!("Testing_tokens@example.com"),
            cur: String::from("USD"),
            typ: u8::from(TokenType::Access),
            slt: 10000,
        };

        let claims_different = TokenClaims {
            exp: 123456788,
            uid: uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap(),
            eml: format!("Testing_tokens@example.com"),
            cur: String::from("USD"),
            typ: u8::from(TokenType::Access),
            slt: 10000,
        };

        let token = claims.create_token(env::CONF.keys.token_signing_key.as_bytes());
        let token_different =
            claims_different.create_token(env::CONF.keys.token_signing_key.as_bytes());
        let expected_token = String::from("eyJleHAiOjEyMzQ1Njc4OSwidWlkIjoiNjdlNTUwNDQtMTBiMS00MjZmLTkyNDctYmI2ODBlNWZlMGM4IiwiZW1sIjoiVGVzdGluZ190b2tlbnNAZXhhbXBsZS5jb20iLCJjdXIiOiJVU0QiLCJ0eXAiOjAsInNsdCI6MTAwMDB9fDY0OWYyNDBkNzZiYzRhOThhMTYzMzc5Y2VhZTdhZDBkNzAwOTgwNWMzYzVlMDlmMzkyMjRjNmM5NGEzZGVlN2Q");

        assert_eq!(token, expected_token);
        assert_ne!(token, token_different);

        let decoded_token =
            base64::decode_config(token.as_bytes(), base64::URL_SAFE_NO_PAD).unwrap();
        let token_str = String::from_utf8_lossy(&decoded_token);
        let split_token = token_str.split('|').collect::<Vec<_>>();

        let mut claims_json_str = String::new();
        for i in 0..(split_token.len() - 1) {
            claims_json_str.push_str(split_token[i]);
        }

        let decoded_claims = serde_json::from_str::<TokenClaims>(claims_json_str.as_str()).unwrap();

        assert_eq!(decoded_claims.exp, claims.exp);
        assert_eq!(decoded_claims.uid, claims.uid);
        assert_eq!(decoded_claims.eml, claims.eml);
        assert_eq!(decoded_claims.cur, claims.cur);
        assert_eq!(decoded_claims.typ, claims.typ);
        assert_eq!(decoded_claims.slt, claims.slt);
    }

    #[actix_rt::test]
    async fn test_claims_from_token_with_validation() {
        let claims = TokenClaims {
            exp: u64::MAX,
            uid: uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap(),
            eml: format!("Testing_tokens@example.com"),
            cur: String::from("USD"),
            typ: u8::from(TokenType::Access),
            slt: 10000,
        };

        let token = claims.create_token(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        let result = TokenClaims::from_token_with_validation(
            &token,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        );

        assert!(result.is_ok());

        let decoded_claims = result.unwrap();

        assert_eq!(decoded_claims.exp, claims.exp);
        assert_eq!(decoded_claims.uid, claims.uid);
        assert_eq!(decoded_claims.eml, claims.eml);
        assert_eq!(decoded_claims.cur, claims.cur);
        assert_eq!(decoded_claims.typ, claims.typ);
        assert_eq!(decoded_claims.slt, claims.slt);
    }

    #[actix_rt::test]
    async fn test_token_validation_fails_with_wrong_key() {
        let claims = TokenClaims {
            exp: u64::MAX,
            uid: uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap(),
            eml: format!("Testing_tokens@example.com"),
            cur: String::from("USD"),
            typ: u8::from(TokenType::Access),
            slt: 10000,
        };

        let token = claims.create_token(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        let result = TokenClaims::from_token_with_validation(
            &token,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17],
        );

        let error = result.unwrap_err();

        assert_eq!(
            std::mem::discriminant(&error),
            std::mem::discriminant(&TokenError::TokenInvalid)
        );
    }

    #[actix_rt::test]
    async fn test_token_validation_fails_when_expired() {
        let claims = TokenClaims {
            exp: 1657076995,
            uid: uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap(),
            eml: format!("Testing_tokens@example.com"),
            cur: String::from("USD"),
            typ: u8::from(TokenType::Access),
            slt: 10000,
        };

        let token = claims.create_token(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        let result = TokenClaims::from_token_with_validation(
            &token,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        );

        let error = result.unwrap_err();

        assert_eq!(
            std::mem::discriminant(&error),
            std::mem::discriminant(&TokenError::TokenExpired)
        );
    }

    #[actix_rt::test]
    async fn test_claims_from_token_without_validation() {
        let claims = TokenClaims {
            exp: 1657076995,
            uid: uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap(),
            eml: format!("Testing_tokens@example.com"),
            cur: String::from("USD"),
            typ: u8::from(TokenType::Access),
            slt: 10000,
        };

        let token = claims.create_token(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        let decoded_claims = TokenClaims::from_token_without_validation(&token).unwrap();

        assert_eq!(decoded_claims.exp, claims.exp);
        assert_eq!(decoded_claims.uid, claims.uid);
        assert_eq!(decoded_claims.eml, claims.eml);
        assert_eq!(decoded_claims.cur, claims.cur);
        assert_eq!(decoded_claims.typ, claims.typ);
        assert_eq!(decoded_claims.slt, claims.slt);
    }

    #[actix_rt::test]
    async fn test_generate_access_token() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let token = generate_access_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert!(!token.token.contains(&user_id.to_string()));

        let decoded_token = TokenClaims::from_token_with_validation(
            &token.token,
            env::CONF.keys.token_signing_key.as_bytes(),
        )
        .unwrap();

        assert_eq!(decoded_token.typ, u8::from(TokenType::Access));
        assert_eq!(decoded_token.uid, user_id);
        assert_eq!(decoded_token.eml, new_user.email);
        assert_eq!(decoded_token.cur, new_user.currency);
        assert!(
            decoded_token.exp
                > SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
        );
    }

    #[actix_rt::test]
    async fn test_generate_refresh_token() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let token = generate_refresh_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert!(!token.token.contains(&user_id.to_string()));

        let decoded_token = TokenClaims::from_token_with_validation(
            &token.token,
            env::CONF.keys.token_signing_key.as_bytes(),
        )
        .unwrap();

        assert_eq!(decoded_token.typ, u8::from(TokenType::Refresh));
        assert_eq!(decoded_token.uid, user_id);
        assert_eq!(decoded_token.eml, new_user.email);
        assert_eq!(decoded_token.cur, new_user.currency);
        assert!(
            decoded_token.exp
                > SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
        );
    }

    #[actix_rt::test]
    async fn test_generate_signin_token() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let token = generate_signin_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert!(!token.token.contains(&user_id.to_string()));

        let decoded_token = TokenClaims::from_token_with_validation(
            &token.token,
            env::CONF.keys.token_signing_key.as_bytes(),
        )
        .unwrap();

        assert_eq!(decoded_token.typ, u8::from(TokenType::SignIn));
        assert_eq!(decoded_token.uid, user_id);
        assert_eq!(decoded_token.eml, new_user.email);
        assert_eq!(decoded_token.cur, new_user.currency);
        assert!(
            decoded_token.exp
                > SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
        );
    }

    #[actix_rt::test]
    async fn test_generate_token_pair() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let token = generate_token_pair(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert!(!token.access_token.token.contains(&user_id.to_string()));
        assert!(!token.refresh_token.token.contains(&user_id.to_string()));

        let decoded_access_token = TokenClaims::from_token_with_validation(
            &token.access_token.token,
            env::CONF.keys.token_signing_key.as_bytes(),
        )
        .unwrap();

        assert_eq!(decoded_access_token.typ, u8::from(TokenType::Access));
        assert_eq!(decoded_access_token.uid, user_id);
        assert_eq!(decoded_access_token.eml, new_user.email);
        assert_eq!(decoded_access_token.cur, new_user.currency);
        assert!(
            decoded_access_token.exp
                > SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
        );

        let decoded_refresh_token = TokenClaims::from_token_with_validation(
            &token.refresh_token.token,
            env::CONF.keys.token_signing_key.as_bytes(),
        )
        .unwrap();

        assert_eq!(decoded_refresh_token.typ, u8::from(TokenType::Refresh));
        assert_eq!(decoded_refresh_token.uid, user_id);
        assert_eq!(decoded_refresh_token.eml, new_user.email);
        assert_eq!(decoded_refresh_token.cur, new_user.currency);
        assert!(
            decoded_refresh_token.exp
                > SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
        );
    }

    #[actix_rt::test]
    async fn test_generate_token() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let access_token = generate_token(
            TokenParams {
                user_id: &new_user.id,
                user_email: new_user.email,
                user_currency: new_user.currency,
            },
            TokenType::Access,
        )
        .unwrap();
        let refresh_token = generate_token(
            TokenParams {
                user_id: &new_user.id,
                user_email: new_user.email,
                user_currency: new_user.currency,
            },
            TokenType::Refresh,
        )
        .unwrap();
        let signin_token = generate_token(
            TokenParams {
                user_id: &new_user.id,
                user_email: new_user.email,
                user_currency: new_user.currency,
            },
            TokenType::SignIn,
        )
        .unwrap();

        let decoded_access_token = TokenClaims::from_token_with_validation(
            &access_token.token,
            env::CONF.keys.token_signing_key.as_bytes(),
        )
        .unwrap();

        let decoded_refresh_token = TokenClaims::from_token_with_validation(
            &refresh_token.token,
            env::CONF.keys.token_signing_key.as_bytes(),
        )
        .unwrap();

        let decoded_signin_token = TokenClaims::from_token_with_validation(
            &signin_token.token,
            env::CONF.keys.token_signing_key.as_bytes(),
        )
        .unwrap();

        assert_eq!(decoded_access_token.typ, u8::from(TokenType::Access));
        assert_eq!(decoded_access_token.uid, user_id);
        assert_eq!(decoded_access_token.eml, new_user.email);
        assert_eq!(decoded_access_token.cur, new_user.currency);
        assert!(
            decoded_access_token.exp
                > SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
        );

        assert_eq!(decoded_refresh_token.typ, u8::from(TokenType::Refresh));
        assert_eq!(decoded_refresh_token.uid, user_id);
        assert_eq!(decoded_refresh_token.eml, new_user.email);
        assert_eq!(decoded_refresh_token.cur, new_user.currency);
        assert!(
            decoded_refresh_token.exp
                > SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
        );

        assert_eq!(decoded_signin_token.typ, u8::from(TokenType::SignIn));
        assert_eq!(decoded_signin_token.uid, user_id);
        assert_eq!(decoded_signin_token.eml, new_user.email);
        assert_eq!(decoded_signin_token.cur, new_user.currency);
        assert!(
            decoded_signin_token.exp
                > SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
        );
    }

    #[actix_rt::test]
    async fn test_validate_access_token() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let access_token = generate_access_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let refresh_token = generate_refresh_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let signin_token = generate_signin_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert_eq!(
            validate_access_token(&access_token.token).unwrap().uid,
            user_id
        );
        assert!(validate_access_token(&refresh_token.token).is_err());
        assert!(validate_access_token(&signin_token.token).is_err());
    }

    #[actix_rt::test]
    async fn test_validate_refresh_token() {
        let db_thread_pool = &*env::testing::DB_THREAD_POOL;
        let db_connection = db_thread_pool.get().unwrap();

        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let access_token = generate_access_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let refresh_token = generate_refresh_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let signin_token = generate_signin_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert_eq!(
            validate_refresh_token(&refresh_token.token, &db_connection)
                .unwrap()
                .uid,
            user_id
        );
        assert!(validate_refresh_token(&access_token.token, &db_connection).is_err());
        assert!(validate_refresh_token(&signin_token.token, &db_connection).is_err());
    }

    #[actix_rt::test]
    async fn test_validate_signin_token() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let access_token = generate_access_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let refresh_token = generate_refresh_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let signin_token = generate_signin_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert_eq!(
            validate_signin_token(&signin_token.token).unwrap().uid,
            user_id
        );
        assert!(validate_signin_token(&access_token.token).is_err());
        assert!(validate_signin_token(&refresh_token.token).is_err());
    }

    #[actix_rt::test]
    async fn test_validate_token() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let access_token = generate_access_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let refresh_token = generate_refresh_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let signin_token = generate_signin_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert_eq!(
            validate_token(&access_token.token, TokenType::Access)
                .unwrap()
                .uid,
            user_id
        );
        assert_eq!(
            validate_token(&refresh_token.token, TokenType::Refresh)
                .unwrap()
                .uid,
            user_id
        );
        assert_eq!(
            validate_token(&signin_token.token, TokenType::SignIn)
                .unwrap()
                .uid,
            user_id
        );
    }

    #[actix_rt::test]
    async fn test_validate_tokens_does_not_validate_tokens_of_wrong_type() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let access_token = generate_access_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let refresh_token = generate_refresh_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let signin_token = generate_signin_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert!(validate_token(&access_token.token, TokenType::SignIn).is_err());
        assert!(validate_token(&refresh_token.token, TokenType::Access).is_err());
        assert!(validate_token(&signin_token.token, TokenType::Refresh).is_err());
    }

    #[actix_rt::test]
    async fn test_read_claims() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let access_token = generate_access_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let refresh_token = generate_refresh_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();
        let signin_token = generate_signin_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        let access_token_claims =
            TokenClaims::from_token_without_validation(&access_token.to_string()).unwrap();
        let refresh_token_claims =
            TokenClaims::from_token_without_validation(&refresh_token.to_string()).unwrap();
        let signin_token_claims =
            TokenClaims::from_token_without_validation(&signin_token.to_string()).unwrap();

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        assert_eq!(access_token_claims.uid, user_id);
        assert_eq!(access_token_claims.typ, u8::from(TokenType::Access));
        assert!(access_token_claims.exp > current_time);

        assert_eq!(refresh_token_claims.uid, user_id);
        assert_eq!(refresh_token_claims.typ, u8::from(TokenType::Refresh));
        assert!(refresh_token_claims.exp > current_time);

        assert_eq!(signin_token_claims.uid, user_id);
        assert_eq!(signin_token_claims.typ, u8::from(TokenType::SignIn));
        assert!(signin_token_claims.exp > current_time);
    }

    #[actix_rt::test]
    async fn test_blacklist_token() {
        let db_thread_pool = &*env::testing::DB_THREAD_POOL;
        let db_connection = db_thread_pool.get().unwrap();

        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        dsl::insert_into(users)
            .values(&new_user)
            .execute(&db_connection)
            .unwrap();

        let refresh_token = generate_refresh_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        let blacklist_token = blacklist_token(&refresh_token.token, &db_connection).unwrap();

        assert_eq!(&blacklist_token.token, &refresh_token.token);
        assert!(
            blacklist_token.token_expiration_time
                > SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
        );

        blacklisted_tokens
            .filter(blacklisted_token_fields::token.eq(&refresh_token.token))
            .get_result::<BlacklistedToken>(&db_connection)
            .unwrap();
    }

    #[actix_rt::test]
    async fn test_is_token_on_blacklist() {
        let db_thread_pool = &*env::testing::DB_THREAD_POOL;
        let db_connection = db_thread_pool.get().unwrap();

        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        dsl::insert_into(users)
            .values(&new_user)
            .execute(&db_connection)
            .unwrap();

        let refresh_token = generate_refresh_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert!(!is_on_blacklist(&refresh_token.token, &db_connection).unwrap());

        blacklist_token(&refresh_token.token, &db_connection).unwrap();

        assert!(is_on_blacklist(&refresh_token.token, &db_connection).unwrap());
    }

    #[actix_rt::test]
    async fn test_is_access_token() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let access_token = generate_access_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert!(access_token.is_access_token());
        assert!(!access_token.is_refresh_token());
        assert!(!access_token.is_signin_token());
    }

    #[actix_rt::test]
    async fn test_is_refresh_token() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let refresh_token = generate_refresh_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert!(refresh_token.is_refresh_token());
        assert!(!refresh_token.is_access_token());
        assert!(!refresh_token.is_signin_token());
    }

    #[actix_rt::test]
    async fn test_is_signin_token() {
        let user_id = Uuid::new_v4();
        let user_number = rand::thread_rng().gen_range::<u128, _>(10_000_000..100_000_000);
        let timestamp = chrono::Utc::now().naive_utc();
        let new_user = NewUser {
            id: user_id,
            is_active: true,
            is_premium: false,
            premium_expiration: Option::None,
            email: &format!("test_user{}@test.com", &user_number),
            password_hash: "test_hash",
            first_name: &format!("Test-{}", &user_number),
            last_name: &format!("User-{}", &user_number),
            date_of_birth: NaiveDate::from_ymd(
                rand::thread_rng().gen_range(1950..=2020),
                rand::thread_rng().gen_range(1..=12),
                rand::thread_rng().gen_range(1..=28),
            ),
            currency: "USD",
            modified_timestamp: timestamp,
            created_timestamp: timestamp,
        };

        let signin_token = generate_signin_token(TokenParams {
            user_id: &new_user.id,
            user_email: new_user.email,
            user_currency: new_user.currency,
        })
        .unwrap();

        assert!(signin_token.is_signin_token());
        assert!(!signin_token.is_access_token());
        assert!(!signin_token.is_refresh_token());
    }
}
