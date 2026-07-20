use codexmanager_core::auth::{DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use codexmanager_core::rpc::types::{
    UsageResetCreditResult, UsageResetCreditsConsumeResult, UsageResetCreditsResult,
};
use codexmanager_core::storage::{Storage, Token};
use rand::RngCore;

use crate::account_proxy::{resolve_account_proxy_mode_from_storage, AccountProxyMode};
use crate::storage_helpers::open_storage;
use crate::usage_account_meta::{derive_account_meta, resolve_workspace_id_for_account};
use crate::usage_http::{
    consume_usage_reset_credit, consume_usage_reset_credit_with_explicit_proxy,
    fetch_usage_reset_credits, fetch_usage_reset_credits_with_explicit_proxy,
    log_account_data_route,
};
use crate::usage_token_refresh::{refresh_and_persist_access_token, token_refresh_ahead_secs};

const DEFAULT_USAGE_BASE_URL: &str = "https://chatgpt.com";

pub(crate) fn read_usage_reset_credits(
    account_id: &str,
) -> Result<UsageResetCreditsResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut token = load_account_token(&storage, account_id)?;
    let workspace_id = resolve_workspace_id(&storage, &token);
    let base_url = usage_base_url();
    let proxy_mode = resolve_account_proxy_mode_from_storage(&storage, account_id);
    log_account_data_route(
        "usage_reset_credits",
        account_id,
        &proxy_mode,
        "rate_limit_reset_credits",
        true,
    );

    let payload = request_with_token_refresh(&storage, &mut token, |current| {
        fetch_reset_credits_for_proxy_mode(
            &base_url,
            &current.access_token,
            workspace_id.as_deref(),
            &proxy_mode,
        )
    })?;
    normalize_reset_credits_payload(&payload)
}

pub(crate) fn consume_usage_reset_credit_for_account(
    account_id: &str,
) -> Result<UsageResetCreditsConsumeResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut token = load_account_token(&storage, account_id)?;
    let workspace_id = resolve_workspace_id(&storage, &token);
    let base_url = usage_base_url();
    let redeem_request_id = create_redeem_request_id();
    let proxy_mode = resolve_account_proxy_mode_from_storage(&storage, account_id);
    log_account_data_route(
        "usage_reset_credit_consume",
        account_id,
        &proxy_mode,
        "rate_limit_reset_credits_consume",
        true,
    );

    request_with_token_refresh(&storage, &mut token, |current| {
        consume_reset_credit_for_proxy_mode(
            &base_url,
            &current.access_token,
            workspace_id.as_deref(),
            &redeem_request_id,
            &proxy_mode,
        )
    })?;

    let usage_refresh_error = crate::usage_refresh::refresh_usage_for_account(account_id).err();
    let reset_credits = fetch_reset_credits_for_proxy_mode(
        &base_url,
        &token.access_token,
        workspace_id.as_deref(),
        &proxy_mode,
    )
    .and_then(|payload| normalize_reset_credits_payload(&payload));

    let mut warnings = Vec::new();
    if let Some(err) = usage_refresh_error {
        warnings.push(format!("usage refresh failed after reset: {err}"));
    }
    let reset_credits = match reset_credits {
        Ok(value) => Some(value),
        Err(err) => {
            warnings.push(format!("reset credit refresh failed after reset: {err}"));
            None
        }
    };

    Ok(UsageResetCreditsConsumeResult {
        reset_applied: true,
        reset_credits,
        usage_refreshed: warnings
            .iter()
            .all(|warning| !warning.starts_with("usage refresh failed")),
        warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
    })
}

fn fetch_reset_credits_for_proxy_mode(
    base_url: &str,
    bearer: &str,
    workspace_id: Option<&str>,
    proxy_mode: &AccountProxyMode,
) -> Result<serde_json::Value, String> {
    match proxy_mode {
        AccountProxyMode::Disabled => fetch_usage_reset_credits(base_url, bearer, workspace_id),
        AccountProxyMode::Explicit { proxy_url, .. } => {
            fetch_usage_reset_credits_with_explicit_proxy(base_url, bearer, workspace_id, proxy_url)
        }
        AccountProxyMode::Invalid { error, .. } => Err(error.clone()),
    }
}

fn consume_reset_credit_for_proxy_mode(
    base_url: &str,
    bearer: &str,
    workspace_id: Option<&str>,
    redeem_request_id: &str,
    proxy_mode: &AccountProxyMode,
) -> Result<(), String> {
    match proxy_mode {
        AccountProxyMode::Disabled => {
            consume_usage_reset_credit(base_url, bearer, workspace_id, redeem_request_id)
        }
        AccountProxyMode::Explicit { proxy_url, .. } => {
            consume_usage_reset_credit_with_explicit_proxy(
                base_url,
                bearer,
                workspace_id,
                redeem_request_id,
                proxy_url,
            )
        }
        AccountProxyMode::Invalid { error, .. } => Err(error.clone()),
    }
}

fn load_account_token(storage: &Storage, account_id: &str) -> Result<Token, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("account_id is required".to_string());
    }
    storage
        .find_token_by_account_id(account_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "account token not found".to_string())
}

fn resolve_workspace_id(storage: &Storage, token: &Token) -> Option<String> {
    resolve_workspace_id_for_account(storage, &token.account_id).or_else(|| {
        let (chatgpt_account_id, workspace_id) = derive_account_meta(token);
        workspace_id.or(chatgpt_account_id)
    })
}

fn usage_base_url() -> String {
    std::env::var("CODEXMANAGER_USAGE_BASE_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_USAGE_BASE_URL.to_string())
}

fn request_with_token_refresh<T, F>(
    storage: &Storage,
    token: &mut Token,
    request: F,
) -> Result<T, String>
where
    F: Fn(&Token) -> Result<T, String>,
{
    match request(token) {
        Ok(value) => Ok(value),
        Err(err) if should_refresh_token(&err) && !token.refresh_token.trim().is_empty() => {
            let issuer =
                std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
            let client_id = std::env::var("CODEXMANAGER_CLIENT_ID")
                .unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
            refresh_and_persist_access_token(
                storage,
                token,
                &issuer,
                &client_id,
                token_refresh_ahead_secs(),
            )?;
            request(token)
        }
        Err(err) => Err(err),
    }
}

fn should_refresh_token(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("status=401")
        || normalized.contains("status=403")
        || normalized.contains("status 401")
        || normalized.contains("status 403")
}

fn normalize_reset_credits_payload(
    payload: &serde_json::Value,
) -> Result<UsageResetCreditsResult, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "invalid usage reset credits response".to_string())?;
    if !object.contains_key("credits")
        && !object.contains_key("available_count")
        && !object.contains_key("availableCount")
    {
        return Err("invalid usage reset credits response".to_string());
    }

    let available_count = object
        .get("available_count")
        .or_else(|| object.get("availableCount"))
        .and_then(json_i64);
    let credits = object
        .get("credits")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(normalize_credit)
        .collect::<Vec<_>>();
    let available_count = available_count.or_else(|| {
        (!credits.is_empty()).then(|| i64::try_from(credits.len()).unwrap_or(i64::MAX))
    });

    Ok(UsageResetCreditsResult {
        available_count,
        credits,
    })
}

fn normalize_credit(value: &serde_json::Value) -> Option<UsageResetCreditResult> {
    let object = value.as_object()?;
    let reset_type = json_string(object.get("reset_type").or_else(|| object.get("resetType")));
    let status = json_string(object.get("status"));
    if reset_type.as_deref() != Some("codex_rate_limits") || status.as_deref() != Some("available")
    {
        return None;
    }
    let expires_at = json_string(object.get("expires_at").or_else(|| object.get("expiresAt")))?;

    Some(UsageResetCreditResult {
        id: json_string(object.get("id")).unwrap_or_default(),
        status: status.unwrap_or_default(),
        granted_at: json_string(object.get("granted_at").or_else(|| object.get("grantedAt")))
            .unwrap_or_default(),
        expires_at,
    })
}

fn json_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(value) => {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        }
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn json_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
}

fn create_redeem_request_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_available_codex_reset_credits() {
        let payload = serde_json::json!({
            "available_count": "2",
            "credits": [
                {
                    "id": "credit-1",
                    "reset_type": "codex_rate_limits",
                    "status": "available",
                    "granted_at": "2026-07-01T00:00:00Z",
                    "expires_at": "2026-08-01T00:00:00Z"
                },
                {
                    "id": "spent",
                    "reset_type": "codex_rate_limits",
                    "status": "consumed",
                    "expires_at": "2026-08-01T00:00:00Z"
                }
            ]
        });

        let result = normalize_reset_credits_payload(&payload).expect("normalize payload");
        assert_eq!(result.available_count, Some(2));
        assert_eq!(result.credits.len(), 1);
        assert_eq!(result.credits[0].id, "credit-1");
    }

    #[test]
    fn redeem_request_id_is_uuid_v4_shaped() {
        let value = create_redeem_request_id();
        assert_eq!(value.len(), 36);
        assert_eq!(&value[14..15], "4");
        assert!(matches!(&value[19..20], "8" | "9" | "a" | "b"));
    }
}
