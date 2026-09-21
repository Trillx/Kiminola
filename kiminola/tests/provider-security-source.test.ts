import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// Wiring tripwires only, not a substitute for cargo test --lib llm::credential_tests.
// These checks read source; they never open the credential store or make requests.
const source = readFileSync(new URL("../src-tauri/src/llm.rs", import.meta.url), "utf8");
const production = source.slice(0, source.indexOf("#[cfg(test)]"));

test("all backend key reads are scoped and have no global-account fallback", () => {
  assert.doesNotMatch(production, /const KEYRING_ACCOUNT:/);
  assert.doesNotMatch(production, /load_api_key\(\)/);
  assert.match(production, /keyring::Entry::new\(KEYRING_SERVICE, account\)/);
  assert.match(production, /store\.read\(&credential_account\(config\)\?\)/);
  assert.match(production, /load_api_key\(config, store\)/);
  assert.match(production, /load_api_key\(&config, store\)/);
});

test("the configured destination and credential share endpoint normalization", () => {
  assert.match(production, /base_url: normalized_base_url\(config\)\?/);
  assert.match(production, /serde_json::to_vec\(&\(config.kind, normalized_base_url\(config\)\?\)\)/);
  assert.match(production, /redirect\(reqwest::redirect::Policy::none\(\)\)/);
});

test("credential persistence precedes activating a provider configuration", () => {
  const setter = production.slice(production.indexOf("async fn set_llm_config_impl("));
  assert.match(setter, /save_api_key\(config, api_key, store\)\?;\s*save_config\(pool, config\)\.await/);
});
