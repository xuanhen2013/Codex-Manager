use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse, ProxyProfileListResult};

use crate::{
    add_proxy_pool_members, cancel_proxy_test_job, create_proxy_pool, create_proxy_profile,
    delete_proxy_pool, delete_proxy_profile, get_proxy_profile_diagnostics_history,
    get_proxy_profile_latency_test_history, get_proxy_profile_speed_test_history,
    get_proxy_test_job, import_proxy_profiles_batch, list_proxy_pool_switch_logs, list_proxy_pools,
    list_proxy_profiles, proxy_test_presets, remove_proxy_pool_member,
    run_proxy_pool_health_cycle_now, set_proxy_pool_member_enabled, test_proxy_profile,
    test_proxy_profile_cloudflare_style_speed, test_proxy_profile_latency,
    test_proxy_profile_speed, update_proxy_pool, update_proxy_profile,
};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "system/proxy/list" => super::value_or_error(
            list_proxy_profiles().map(|items| ProxyProfileListResult { items }),
        ),
        "system/proxy/create" => super::value_or_error(create_proxy_profile(
            super::string_param(req, "name"),
            first_string_param(req, &["proxyUrl", "proxy_url"]),
            super::bool_param(req, "enabled"),
            first_string_param(req, &["tagsJson", "tags_json"]),
            super::string_param(req, "notes"),
        )),
        "system/proxy/update" => super::value_or_error(update_proxy_profile(
            proxy_profile_id_param(req).unwrap_or(""),
            super::string_param(req, "name"),
            first_string_param(req, &["proxyUrl", "proxy_url"]),
            super::bool_param(req, "enabled"),
            first_string_param(req, &["tagsJson", "tags_json"]),
            super::string_param(req, "notes"),
        )),
        "system/proxy/delete" => super::ok_or_error(delete_proxy_profile(
            proxy_profile_id_param(req).unwrap_or(""),
        )),
        "system/proxy/test" => super::value_or_error(test_proxy_profile(
            proxy_profile_id_param(req).unwrap_or(""),
        )),
        "system/proxy/test-latency" => super::value_or_error(test_proxy_profile_latency(
            proxy_profile_id_param(req).unwrap_or(""),
        )),
        "system/proxy/speed-test" => super::value_or_error(test_proxy_profile_speed(
            proxy_profile_id_param(req).unwrap_or(""),
            super::str_param(req, "providerId").or_else(|| super::str_param(req, "provider_id")),
            super::str_param(req, "fileSizeId").or_else(|| super::str_param(req, "file_size_id")),
            super::str_param(req, "diagnosticProviderId")
                .or_else(|| super::str_param(req, "diagnostic_provider_id")),
            super::str_param(req, "diagnosticFileSizeId")
                .or_else(|| super::str_param(req, "diagnostic_file_size_id")),
        )),
        "system/proxy/cloudflare-speed-test" => {
            let config = req
                .params
                .as_ref()
                .and_then(|p| p.get("config"))
                .cloned()
                .map(
                    serde_json::from_value::<
                        crate::account::proxy_testing::cloudflare_style::config::CfStyleConfig,
                    >,
                )
                .transpose()
                .map_err(|err| format!("invalid config parameter: {err}"));

            match config {
                Ok(conf) => super::value_or_error(test_proxy_profile_cloudflare_style_speed(
                    proxy_profile_id_param(req).unwrap_or(""),
                    conf.unwrap_or_default(),
                )),
                Err(err) => super::value_or_error(Err::<serde_json::Value, String>(err)),
            }
        }

        "system/proxy/test-presets" => super::value_or_error(Ok(proxy_test_presets())),
        "system/proxy/test-job" => {
            super::value_or_error(get_proxy_test_job(job_id_param(req).unwrap_or("")))
        }
        "system/proxy/cancel-test" => {
            super::ok_or_error(cancel_proxy_test_job(job_id_param(req).unwrap_or("")))
        }
        "system/proxy/speed-test-history" => {
            super::value_or_error(get_proxy_profile_speed_test_history(
                proxy_profile_id_param(req).unwrap_or(""),
                super::i64_param(req, "limit").map(|v| v as usize),
            ))
        }
        "system/proxy/latency-test-history" => {
            super::value_or_error(get_proxy_profile_latency_test_history(
                proxy_profile_id_param(req).unwrap_or(""),
                super::i64_param(req, "limit").map(|v| v as usize),
            ))
        }
        "system/proxy/diagnostics-history" => {
            super::value_or_error(get_proxy_profile_diagnostics_history(
                proxy_profile_id_param(req).unwrap_or(""),
                super::i64_param(req, "limit").map(|v| v as usize),
            ))
        }
        "system/proxy/import-batch" => {
            let parsed = req
                .params
                .clone()
                .ok_or_else(|| "params are required".to_string())
                .and_then(|value| {
                    serde_json::from_value::<crate::proxy_pool::ProxyBatchImportRequest>(value)
                        .map_err(|err| format!("invalid batch import params: {err}"))
                });
            match parsed {
                Ok(request) => super::value_or_error(import_proxy_profiles_batch(request)),
                Err(err) => super::value_or_error(Err::<serde_json::Value, String>(err)),
            }
        }
        "system/proxy-pool/list" => super::value_or_error(list_proxy_pools()),
        "system/proxy-pool/create" => super::value_or_error(create_proxy_pool(
            super::string_param(req, "name"),
            super::string_param(req, "description"),
            super::bool_param(req, "enabled"),
            super::i64_param(req, "failureThreshold"),
            super::i64_param(req, "recoveryThreshold"),
            super::i64_param(req, "heartbeatIntervalSecs"),
            super::i64_param(req, "cooldownSecs"),
        )),
        "system/proxy-pool/update" => super::value_or_error(update_proxy_pool(
            pool_id_param(req).unwrap_or(""),
            super::string_param(req, "name"),
            super::string_param(req, "description"),
            super::bool_param(req, "enabled"),
            super::i64_param(req, "failureThreshold"),
            super::i64_param(req, "recoveryThreshold"),
            super::i64_param(req, "heartbeatIntervalSecs"),
            super::i64_param(req, "cooldownSecs"),
        )),
        "system/proxy-pool/delete" => {
            super::ok_or_error(delete_proxy_pool(pool_id_param(req).unwrap_or("")))
        }
        "system/proxy-pool/members/add" => super::value_or_error(add_proxy_pool_members(
            pool_id_param(req).unwrap_or(""),
            string_array_param(req, "proxyProfileIds"),
        )),
        "system/proxy-pool/members/remove" => super::value_or_error(remove_proxy_pool_member(
            pool_id_param(req).unwrap_or(""),
            proxy_profile_id_param(req).unwrap_or(""),
        )),
        "system/proxy-pool/members/set-enabled" => {
            super::value_or_error(set_proxy_pool_member_enabled(
                pool_id_param(req).unwrap_or(""),
                proxy_profile_id_param(req).unwrap_or(""),
                super::bool_param(req, "enabled").unwrap_or(true),
            ))
        }
        "system/proxy-pool/health-check" => {
            super::value_or_error(run_proxy_pool_health_cycle_now())
        }
        "system/proxy-pool/switch-logs" => super::value_or_error(list_proxy_pool_switch_logs(
            super::str_param(req, "accountId"),
            super::i64_param(req, "limit").map(|value| value as usize),
        )),
        _ => return None,
    };

    Some(super::response(req, result))
}

fn proxy_profile_id_param<'a>(req: &'a JsonRpcRequest) -> Option<&'a str> {
    super::str_param(req, "id").or_else(|| super::str_param(req, "proxyId"))
}

fn pool_id_param<'a>(req: &'a JsonRpcRequest) -> Option<&'a str> {
    super::str_param(req, "poolId").or_else(|| super::str_param(req, "pool_id"))
}

fn string_array_param(req: &JsonRpcRequest, key: &str) -> Vec<String> {
    req.params
        .as_ref()
        .and_then(|params| params.get(key))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn job_id_param<'a>(req: &'a JsonRpcRequest) -> Option<&'a str> {
    super::str_param(req, "jobId")
        .or_else(|| super::str_param(req, "job_id"))
        .or_else(|| super::str_param(req, "id"))
}

fn first_string_param(req: &JsonRpcRequest, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| super::str_param(req, key).map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::try_handle;
    use codexmanager_core::rpc::types::JsonRpcRequest;

    fn rpc_request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
        JsonRpcRequest {
            id: 1.into(),
            method: method.to_string(),
            params: Some(params),
            trace: None,
        }
    }

    fn error_message(resp: &codexmanager_core::rpc::types::JsonRpcResponse) -> String {
        resp.result
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn proxy_update_accepts_id_and_proxy_id() {
        let missing = try_handle(&rpc_request("system/proxy/update", serde_json::json!({})))
            .expect("response");
        assert_eq!(error_message(&missing), "id is required");

        let with_id = try_handle(&rpc_request(
            "system/proxy/update",
            serde_json::json!({ "id": "pp_test", "name": "Proxy" }),
        ))
        .expect("response");
        assert_ne!(error_message(&with_id), "id is required");

        let with_proxy_id = try_handle(&rpc_request(
            "system/proxy/update",
            serde_json::json!({ "proxyId": "pp_test", "name": "Proxy" }),
        ))
        .expect("response");
        assert_ne!(error_message(&with_proxy_id), "id is required");
    }

    #[test]
    fn proxy_test_presets_route_returns_defaults() {
        let resp = try_handle(&rpc_request(
            "system/proxy/test-presets",
            serde_json::json!({}),
        ))
        .expect("response");
        assert_eq!(
            resp.result["defaults"]["speedProviderId"].as_str(),
            Some("cloudflare_http_rust")
        );
        assert_eq!(
            resp.result["defaults"]["fileSizeId"].as_str(),
            Some("size_25mb")
        );
        assert!(resp.result["uploadEndpoint"].get("configured").is_some());
    }
}
