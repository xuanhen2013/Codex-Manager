import type { WebCommandDescriptor } from "./shared";

export function createProxyProfilesWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_system_proxy_list: { rpcMethod: "system/proxy/list" },
    service_system_proxy_test_presets: { rpcMethod: "system/proxy/test-presets" },
    service_system_proxy_create: { rpcMethod: "system/proxy/create" },
    service_system_proxy_update: { rpcMethod: "system/proxy/update" },
    service_system_proxy_delete: { rpcMethod: "system/proxy/delete" },
    service_system_proxy_test_latency: { rpcMethod: "system/proxy/test-latency" },
    service_system_proxy_speed_test: { rpcMethod: "system/proxy/speed-test" },
    service_system_proxy_cloudflare_speed_test: { rpcMethod: "system/proxy/cloudflare-speed-test" },
    service_system_proxy_test_job: { rpcMethod: "system/proxy/test-job" },
    service_system_proxy_cancel_test: { rpcMethod: "system/proxy/cancel-test" },
    service_system_proxy_speed_test_history: { rpcMethod: "system/proxy/speed-test-history" },
    service_system_proxy_latency_test_history: { rpcMethod: "system/proxy/latency-test-history" },
    service_system_proxy_diagnostics_history: { rpcMethod: "system/proxy/diagnostics-history" },
    service_system_proxy_import_batch: { rpcMethod: "system/proxy/import-batch" },
    service_system_proxy_pool_list: { rpcMethod: "system/proxy-pool/list" },
    service_system_proxy_pool_create: { rpcMethod: "system/proxy-pool/create" },
    service_system_proxy_pool_update: { rpcMethod: "system/proxy-pool/update" },
    service_system_proxy_pool_delete: { rpcMethod: "system/proxy-pool/delete" },
    service_system_proxy_pool_members_add: { rpcMethod: "system/proxy-pool/members/add" },
    service_system_proxy_pool_members_remove: { rpcMethod: "system/proxy-pool/members/remove" },
    service_system_proxy_pool_members_set_enabled: { rpcMethod: "system/proxy-pool/members/set-enabled" },
    service_system_proxy_pool_health_check: { rpcMethod: "system/proxy-pool/health-check" },
    service_system_proxy_pool_switch_logs: { rpcMethod: "system/proxy-pool/switch-logs" },
  };
}
