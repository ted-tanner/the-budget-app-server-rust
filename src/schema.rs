table! {
    blacklisted_tokens (id) {
        id -> Int4,
        token -> Varchar,
        user_id -> Uuid,
        token_expiration_time -> Int8,
    }
}

table! {
    budget_comment_reactions (id) {
        id -> Uuid,
        comment_id -> Uuid,
        user_id -> Uuid,
        reaction -> Int2,
        modified_timestamp -> Timestamp,
        created_timestamp -> Timestamp,
    }
}

table! {
    budget_comments (id) {
        id -> Uuid,
        budget_id -> Uuid,
        user_id -> Uuid,
        is_deleted -> Bool,
        is_current -> Bool,
        text -> Text,
        modified_timestamp -> Timestamp,
        created_timestamp -> Timestamp,
    }
}

table! {
    budget_share_events (id) {
        id -> Uuid,
        recipient_user_id -> Uuid,
        sharer_user_id -> Uuid,
        budget_id -> Uuid,
        accepted -> Bool,
        share_timestamp -> Timestamp,
        accepted_declined_timestamp -> Nullable<Timestamp>,
    }
}

table! {
    budgets (id) {
        id -> Uuid,
        is_shared -> Bool,
        is_private -> Bool,
        is_deleted -> Bool,
        name -> Varchar,
        description -> Nullable<Text>,
        start_date -> Date,
        end_date -> Date,
        latest_entry_time -> Timestamp,
        modified_timestamp -> Timestamp,
        created_timestamp -> Timestamp,
    }
}

table! {
    categories (pk) {
        pk -> Int4,
        budget_id -> Uuid,
        is_deleted -> Bool,
        id -> Int2,
        name -> Varchar,
        limit_cents -> Int8,
        color -> Varchar,
        modified_timestamp -> Timestamp,
        created_timestamp -> Timestamp,
    }
}

table! {
    entries (id) {
        id -> Uuid,
        budget_id -> Uuid,
        user_id -> Uuid,
        is_deleted -> Bool,
        amount_cents -> Int8,
        date -> Date,
        name -> Nullable<Varchar>,
        category -> Nullable<Int2>,
        note -> Nullable<Text>,
        modified_timestamp -> Timestamp,
        created_timestamp -> Timestamp,
    }
}

table! {
    entry_comment_reactions (id) {
        id -> Uuid,
        comment_id -> Uuid,
        user_id -> Uuid,
        reaction -> Int2,
        modified_timestamp -> Timestamp,
        created_timestamp -> Timestamp,
    }
}

table! {
    entry_comments (id) {
        id -> Uuid,
        entry_id -> Uuid,
        user_id -> Uuid,
        is_deleted -> Bool,
        is_current -> Bool,
        text -> Text,
        modified_timestamp -> Timestamp,
        created_timestamp -> Timestamp,
    }
}

table! {
    otp_attempts (user_id) {
        user_id -> Uuid,
        attempt_count -> Int2,
    }
}

table! {
    password_attempts (user_id) {
        user_id -> Uuid,
        attempt_count -> Int2,
    }
}

table! {
    user_budgets (id) {
        id -> Int4,
        created_timestamp -> Timestamp,
        user_id -> Uuid,
        budget_id -> Uuid,
    }
}

table! {
    user_notifications (id) {
        id -> Uuid,
        user_id -> Uuid,
        is_unread -> Bool,
        is_pristine -> Bool,
        is_deleted -> Bool,
        notification_type -> Int2,
        alt_title -> Varchar,
        alt_message -> Varchar,
        associated_data -> Nullable<Text>,
        modified_timestamp -> Timestamp,
        created_timestamp -> Timestamp,
    }
}

table! {
    users (id) {
        id -> Uuid,
        password_hash -> Text,
        is_active -> Bool,
        is_premium -> Bool,
        premium_expiration -> Nullable<Date>,
        email -> Varchar,
        first_name -> Varchar,
        last_name -> Varchar,
        date_of_birth -> Date,
        currency -> Varchar,
        modified_timestamp -> Timestamp,
        created_timestamp -> Timestamp,
    }
}

joinable!(entry_comments -> entries (entry_id));

allow_tables_to_appear_in_same_query!(
    blacklisted_tokens,
    budget_comment_reactions,
    budget_comments,
    budget_share_events,
    budgets,
    categories,
    entries,
    entry_comment_reactions,
    entry_comments,
    otp_attempts,
    password_attempts,
    user_budgets,
    user_notifications,
    users,
);
