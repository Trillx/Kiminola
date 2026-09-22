import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';

const root = new URL('../../', import.meta.url);
const read = (path: string) => readFile(new URL(path, root), 'utf8');

test('CI runs distinct quality, security, and native packaging gates', async () => {
  const [workflow, packageJson] = await Promise.all([
    read('.github/workflows/ci.yml'),
    read('kiminola/package.json'),
  ]);
  const scripts = JSON.parse(packageJson).scripts as Record<string, string>;

  assert.match(workflow, /^  frontend-quality:/m);
  assert.match(workflow, /^  rust-quality:/m);
  assert.match(workflow, /^  package-windows:/m);
  assert.match(workflow, /^  dependency-review:/m);
  assert.match(workflow, /^  cargo-audit:/m);
  assert.match(workflow, /runner: windows-2025/);
  assert.match(workflow, /runner: windows-11-arm/);
  assert.match(workflow, /cargo fmt --all -- --check/);
  assert.match(workflow, /cargo clippy --locked --all-targets/);
  assert.match(workflow, /cargo test --locked --all-targets/);
  assert.match(workflow, /cargo test --locked --doc/);
  assert.match(workflow, /Run all-target Rust tests/);
  assert.match(workflow, /Run Rust documentation tests/);
  assert.doesNotMatch(workflow, /cargo test --locked --lib/);
  assert.match(workflow, /npm\.cmd audit --omit=dev --audit-level=moderate/);
  assert.match(workflow, /LIBCLANG_PATH=/);
  assert.doesNotMatch(workflow, /choco install llvm/i);
  assert.match(workflow, /npm\.cmd run test:browser/);
  assert.match(workflow, /verify-pe-architecture\.ps1/);
  assert.match(workflow, /npm\.cmd run tauri build --[\s\S]*?--no-bundle/);
  assert.match(workflow, /npm\.cmd run tauri bundle --/);
  assert.match(workflow, /stage-portable-package\.ps1/);
  assert.match(workflow, /portable[\\/]Kimi-Nola-\$\{\{ matrix\.arch \}\}-portable\.zip/);
  assert.match(workflow, /actions\/upload-artifact@[0-9a-f]{40}/);
  assert.match(workflow, /cargo-audit:[\s\S]*?checks: write/);
  assert.doesNotMatch(workflow, /windows-latest|@[vV]\d+\b|@stable\b/);

  assert.match(scripts['test:browser'] ?? '', /test:ui/);
  assert.match(scripts['test:browser'] ?? '', /window-resize\.browser\.mjs/);
  assert.match(scripts['test:browser'] ?? '', /sidebar-tree\.browser\.mjs/);
});

test('release validates before creating a draft and builds on native runners', async () => {
  const workflow = await read('.github/workflows/release.yml');
  const preflight = workflow.indexOf('  preflight:');
  const createRelease = workflow.indexOf('  create-release:');

  assert.ok(preflight >= 0, 'release preflight job is present');
  assert.ok(createRelease > preflight, 'release preflight is defined before draft creation');
  assert.match(workflow, /create-release:[\s\S]*?needs:[\s\S]*?- preflight[\s\S]*?- hardware-validation/);
  assert.match(workflow, /runner: windows-2025/);
  assert.match(workflow, /runner: windows-11-arm/);
  assert.match(workflow, /generate-updater-manifest\.test\.ps1/);
  assert.match(workflow, /verify-pe-architecture\.ps1/);
  assert.match(workflow, /^  hardware-validation:/m);
  assert.match(workflow, /uses: \.\/\.github\/workflows\/hardware-validation\.yml/);
  assert.match(workflow, /npm\.cmd run tauri bundle --/);
  assert.match(workflow, /stage-portable-package\.ps1/);
  assert.match(workflow, /release-verification-[^\s'"`]+\.json/);
  assert.match(workflow, /minisign\.exe[\s\S]*?-V/);
  assert.match(workflow, /persist-credentials: false/);
  assert.match(workflow, /uploadedManifestPath[\s\S]*?Get-FileHash/);
  assert.match(workflow, /generatedHash[\s\S]*?uploadedHash[\s\S]*?uploadedHash -ne \$generatedHash/);
  assert.doesNotMatch(workflow, /tauri-apps\/tauri-action/);
  assert.doesNotMatch(workflow, /choco install llvm/i);
  assert.doesNotMatch(workflow, /windows-latest|@[vV]\d+\b|@stable\b/);
});

test('toolchains, dependencies, and hardware validation are explicitly managed', async () => {
  const [toolchain, dependabot, hardware] = await Promise.all([
    read('kiminola/rust-toolchain.toml'),
    read('.github/dependabot.yml'),
    read('.github/workflows/hardware-validation.yml'),
  ]);

  assert.match(toolchain, /channel\s*=\s*"\d+\.\d+\.\d+"/);
  assert.match(toolchain, /components\s*=\s*\["rustfmt", "clippy"\]/);
  assert.match(dependabot, /package-ecosystem: "npm"/);
  assert.match(dependabot, /package-ecosystem: "cargo"/);
  assert.match(dependabot, /package-ecosystem: "github-actions"/);
  assert.match(hardware, /workflow_dispatch:/);
  assert.match(hardware, /schedule:/);
  assert.match(hardware, /group: hardware-validation\s*$/m);
  assert.match(hardware, /classic_loopback_captures_non_silent_audio/);
  assert.match(hardware, /process_loopback_captures_non_silent_audio/);
  assert.match(hardware, /physical_microphone_captures_non_silent_audio/);
  assert.match(hardware, /session_transcribes_speech_wav/);
  assert.match(hardware, /KIMINOLA_REQUIRE_ASR_FIXTURE/);
  assert.match(hardware, /hardware-update-validation\.ps1/);
  assert.match(hardware, /-Target '\$\{\{ matrix\.target \}\}'/);
  assert.match(hardware, /RuntimeInformation.*ProcessArchitecture/);
  assert.match(hardware, /clang\.exe[\s\S]*?clang-cl\.exe[\s\S]*?libclang\.dll[\s\S]*?verify-pe-architecture\.ps1/);
  assert.doesNotMatch(hardware, /^  hardware-not-configured:/m);
  assert.doesNotMatch(hardware, /choco install llvm/i);
  assert.doesNotMatch(hardware, /gh release download/i);
});

test('hardware update validation preserves the provisioned ASR model pack', async () => {
  const script = await read('kiminola/scripts/hardware-update-validation.ps1');

  assert.doesNotMatch(script, /Remove-Item[^\n]*\$userRoot/);
  assert.match(script, /Get-FileHash/);
  assert.match(script, /model hashes changed/i);
  assert.match(script, /hardware_update_database_record_survives/);
  assert.match(script, /\.exe\.sig|minisign/);
  assert.match(script, /Remove-Item Env:GH_TOKEN[\s\S]*?Start-Process -FilePath \$previousInstallerPath/);
  assert.match(script, /Move-Item[^\n]*\$appRoot/);
  assert.match(script, /Move-Item[^\n]*\$dataRoot/);
  assert.match(script, /Move-Item[^\n]*\$appBackup[^\n]*\$appRoot/);
  assert.match(script, /Move-Item[^\n]*\$dataBackup[^\n]*\$dataRoot/);
  assert.doesNotMatch(script, /hardware-update-marker\.txt/);
});
