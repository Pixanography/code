use crate::auth::flows::AuthProvider;
use crate::auth::session::get_session_metadata;
use crate::auth::AuthenticationError;
use crate::database::models::user_item;
use crate::models::users::{Role, User, UserId, UserPayoutData};
use crate::queue::session::SessionQueue;
use actix_web::HttpRequest;
use chrono::Utc;
use reqwest::header::{HeaderValue, AUTHORIZATION};

pub async fn get_user_from_headers<'a, E>(
    req: &HttpRequest,
    executor: E,
    redis: &deadpool_redis::Pool,
    session_queue: &SessionQueue,
) -> Result<User, AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
{
    let headers = req.headers();
    let token: Option<&HeaderValue> = headers.get(AUTHORIZATION);

    // Fetch DB user record and minos user from headers
    let db_user = get_user_record_from_bearer_token(
        req,
        token
            .ok_or_else(|| AuthenticationError::InvalidAuthMethod)?
            .to_str()
            .map_err(|_| AuthenticationError::InvalidCredentials)?,
        executor,
        redis,
        session_queue,
    )
    .await?
    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

    let user = User {
        id: UserId::from(db_user.id),
        github_id: db_user.github_id.map(|x| x as u64),
        // discord_id: minos_user.discord_id,
        // google_id: minos_user.google_id,
        // microsoft_id: minos_user.microsoft_id,
        // apple_id: minos_user.apple_id,
        // gitlab_id: minos_user.gitlab_id,
        username: db_user.username,
        name: db_user.name,
        email: db_user.email,
        avatar_url: db_user.avatar_url,
        bio: db_user.bio,
        created: db_user.created,
        role: Role::from_string(&db_user.role),
        badges: db_user.badges,
        payout_data: Some(UserPayoutData {
            balance: db_user.balance,
            payout_wallet: db_user.payout_wallet,
            payout_wallet_type: db_user.payout_wallet_type,
            payout_address: db_user.payout_address,
        }),
    };
    Ok(user)
}

pub async fn get_user_record_from_bearer_token<'a, 'b, E>(
    req: &HttpRequest,
    token: &str,
    executor: E,
    redis: &deadpool_redis::Pool,
    session_queue: &SessionQueue,
) -> Result<Option<user_item::User>, AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
{
    let possible_user = match token.split_once('_') {
        //Some(("modrinth", _)) => get_user_from_pat(token, executor).await?,
        Some(("mra", _)) => {
            let session =
                crate::database::models::session_item::Session::get(token, executor, redis)
                    .await?
                    .ok_or_else(|| AuthenticationError::InvalidCredentials)?;

            if session.expires < Utc::now() {
                return Err(AuthenticationError::InvalidCredentials);
            }

            let user = user_item::User::get_id(session.user_id, executor, redis).await?;

            let metadata = get_session_metadata(req).await?;
            session_queue.add(session.id, metadata).await;

            user
        }
        Some(("github", _)) | Some(("gho", _)) | Some(("ghp", _)) => {
            let user = AuthProvider::GitHub.get_user(token).await?;
            let id = AuthProvider::GitHub.get_user_id(&user.id, executor).await?;

            user_item::User::get_id(
                id.ok_or_else(|| AuthenticationError::InvalidCredentials)?,
                executor,
                redis,
            )
            .await?
        }
        _ => return Err(AuthenticationError::InvalidAuthMethod),
    };
    Ok(possible_user)
}

pub async fn check_is_moderator_from_headers<'a, 'b, E>(
    req: &HttpRequest,
    executor: E,
    redis: &deadpool_redis::Pool,
    session_queue: &SessionQueue,
) -> Result<User, AuthenticationError>
where
    E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
{
    let user = get_user_from_headers(req, executor, redis, session_queue).await?;

    if user.role.is_mod() {
        Ok(user)
    } else {
        Err(AuthenticationError::InvalidCredentials)
    }
}
