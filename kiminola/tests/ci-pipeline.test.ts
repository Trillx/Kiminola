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
  assert.match(workflow, /push:[\s\S]*?tags:[\s\S]*?- 'v\*'/);
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
  assert.match(workflow, /hardware-runner-state\.test\.ps1/);
  assert.match(workflow, /portable[\\/]Kimi-Nola-\$\{\{ matrix\.arch \}\}-portable\.zip/);
  assert.match(workflow, /actions\/upload-artifact@[0-9a-f]{40}/);
  assert.match(workflow, /cargo-audit:[\s\S]*?checks: write/);
  assert.match(workflow, /cargo-audit:[\s\S]*?ignore: RUSTSEC-2023-0071/);
  assert.doesNotMatch(workflow, /windows-latest|@[vV]\d+\b|@stable\b/);

  assert.match(scripts['test:browser'] ?? '', /test:ui/);
  assert.match(scripts['test:browser'] ?? '', /window-resize\.browser\.mjs/);
  assert.match(scripts['test:browser'] ?? '', /sidebar-tree\.browser\.mjs/);
});

test('release validates before creating a draft and builds on native runners', async () => {
  const workflow = await read('.github/workflows/release.yml');
  const preflight = workflow.indexOf('  preflight:');
  const hardwareGate = workflow.indexOf('  hardware-gate:');
  const createRelease = workflow.indexOf('  create-release:');

  assert.match(workflow, /on:[\s\S]*?push:[\s\S]*?tags:[\s\S]*?'v\*'/);
  assert.match(workflow, /git merge-base --is-ancestor/);
  assert.match(workflow, /refs\/remotes\/origin\/main/);
  assert.ok(preflight >= 0, 'release preflight job is present');
  assert.ok(createRelease > preflight, 'release preflight is defined before draft creation');
  assert.ok(hardwareGate > preflight && hardwareGate < createRelease, 'a hosted hardware result gate runs before draft creation');
  assert.match(workflow, /hardware-gate:[\s\S]*?runs-on: windows-2025/);
  assert.match(workflow, /hardware-gate:[\s\S]*?timeout-minutes: 220/);
  assert.match(workflow, /hardware-gate:[\s\S]*?actions: write/);
  assert.match(workflow, /actions\/workflows\/hardware-validation\.yml\/dispatches/);
  assert.match(workflow, /head_sha[\s\S]*?RELEASE_COMMIT/);
  assert.match(workflow, /RELEASE_RUN_ID[\s\S]*?display_title/);
  assert.match(workflow, /AddMinutes\(210\)/);
  assert.match(workflow, /create-release:[\s\S]*?needs:[\s\S]*?- preflight[\s\S]*?- hardware-gate/);
  assert.match(workflow, /security-gate:[\s\S]*?npm audit --omit=dev --audit-level=moderate/);
  assert.match(workflow, /security-gate:[\s\S]*?rustsec\/audit-check@[0-9a-f]{40}/);
  assert.match(workflow, /security-gate:[\s\S]*?ignore: RUSTSEC-2023-0071/);
  assert.match(workflow, /create-release:[\s\S]*?needs:[\s\S]*?- security-gate/);
  assert.match(workflow, /runner: windows-2025/);
  assert.match(workflow, /runner: windows-11-arm/);
  assert.match(workflow, /generate-updater-manifest\.test\.ps1/);
  assert.match(workflow, /verify-pe-architecture\.ps1/);
  assert.doesNotMatch(workflow, /uses: \.\/\.github\/workflows\/hardware-validation\.yml/);
  assert.match(workflow, /npm\.cmd run tauri bundle --/);
  assert.match(workflow, /SIGNPATH_API_TOKEN/);
  assert.match(workflow, /SIGNPATH_ORGANIZATION_ID/);
  assert.match(workflow, /SIGNPATH_PROJECT_SLUG/);
  assert.match(workflow, /SIGNPATH_SIGNING_POLICY_SLUG/);
  assert.match(workflow, /SIGNPATH_ARTIFACT_CONFIGURATION_SLUG/);
  assert.match(workflow, /SIGNPATH_SIGNER_THUMBPRINT/);
  assert.match(workflow, /\^\[0-9A-Fa-f\]\{40\}\$/);
  const signingPreflight = workflow.indexOf('SIGNPATH_API_TOKEN');
  assert.ok(signingPreflight > preflight && signingPreflight < createRelease, 'signing credentials are checked before draft creation');
  assert.match(workflow, /signpath\/github-action-submit-signing-request@[0-9a-f]{40}/);
  assert.equal(
    workflow.match(/skip-decompress: false/g)?.length,
    2,
    'both SignPath submissions explicitly use the ZIP artifact configuration'
  );
  assert.ok(
    (workflow.match(/EXPECTED_SIGNER_THUMBPRINT/g)?.length ?? 0) >= 6,
    'the independently configured signer is checked after signing and during release readback'
  );
  assert.match(workflow, /EXPECTED_SIGNER_THUMBPRINT\.Trim\(\)\.ToUpperInvariant\(\)/);
  assert.ok(
    (workflow.match(/TimeStamperCertificate/g)?.length ?? 0) >= 6,
    'every signed executable and installer requires an Authenticode timestamp'
  );
  assert.match(workflow, /github-artifact-id:[\s\S]*?wait-for-completion: true[\s\S]*?Get-AuthenticodeSignature/);
  assert.match(workflow, /Get-AuthenticodeSignature[\s\S]*?npm\.cmd run tauri -- signer sign/);
  assert.match(workflow, /\$canonicalInstallerName = "Kimi\.Nola_\$\{version\}_\$\{arch\}-setup\.exe"/);
  assert.match(workflow, /Move-Item -LiteralPath \$installer\.FullName -Destination \$canonicalInstallerPath/);
  const staleSignatureRemoval = workflow.indexOf('Remove-Item -LiteralPath $initialSignaturePath');
  const canonicalInstallerMove = workflow.indexOf('Move-Item -LiteralPath $installer.FullName -Destination $canonicalInstallerPath');
  assert.ok(
    staleSignatureRemoval >= 0 && staleSignatureRemoval < canonicalInstallerMove,
    'the pre-Authenticode updater signature is removed before the installer is renamed'
  );
  assert.match(workflow, /Expected exactly one canonical updater signature/);
  const canonicalRename = workflow.indexOf('$canonicalInstallerName = "Kimi.Nola_${version}_');
  const updaterSigning = workflow.indexOf('npm.cmd run tauri -- signer sign $installer.FullName');
  assert.ok(canonicalRename >= 0 && updaterSigning > canonicalRename, 'canonical release naming precedes updater signing');
  assert.match(workflow, /downloadDirectory[\s\S]*?Get-AuthenticodeSignature/);
  assert.match(workflow, /Expand-Archive[\s\S]*?verify-pe-architecture\.ps1/);
  assert.match(workflow, /\$portableExecutablePath = Join-Path \$portableExtract 'kiminola\.exe'/);
  assert.match(workflow, /Get-AuthenticodeSignature -FilePath \$portableExecutablePath/);
  assert.match(workflow, /portable\.signer_thumbprint/);
  assert.match(workflow, /stage-portable-package\.ps1/);
  assert.match(workflow, /hardware-runner-state\.test\.ps1/);
  assert.match(workflow, /release-verification-[^\s'"`]+\.json/);
  assert.match(workflow, /minisign\.exe[\s\S]*?-V/);
  assert.match(workflow, /FromBase64String\(\(Get-Content -Raw \$signaturePath\)\.Trim\(\)\)/);
  assert.match(workflow, /-x \$rawSignaturePath/);
  assert.match(workflow, /persist-credentials: false/);
  assert.match(workflow, /windows-release:[\s\S]*?permissions:[\s\S]*?contents: read/);
  assert.match(workflow, /manifest:[\s\S]*?permissions:[\s\S]*?contents: read/);
  assert.match(workflow, /publish-assets:[\s\S]*?permissions:[\s\S]*?contents: write/);
  assert.match(workflow, /publish-manifest:[\s\S]*?permissions:[\s\S]*?contents: write/);
  assert.match(workflow, /uploadedManifestPath[\s\S]*?Get-FileHash/);
  assert.match(workflow, /generatedHash[\s\S]*?uploadedHash[\s\S]*?uploadedHash -ne \$generatedHash/);
  assert.doesNotMatch(workflow, /tauri-apps\/tauri-action/);
  assert.doesNotMatch(workflow, /choco install llvm/i);
  assert.doesNotMatch(workflow, /windows-latest|@[vV]\d+\b|@stable\b/);
  assert.doesNotMatch(workflow, /generate signed updater manifest/i);
});

test('toolchains, dependencies, and hardware validation are explicitly managed', async () => {
  const [toolchain, dependabot, hardware, nativePreparation, models] = await Promise.all([
    read('kiminola/rust-toolchain.toml'),
    read('.github/dependabot.yml'),
    read('.github/workflows/hardware-validation.yml'),
    read('kiminola/scripts/prepare-native-deps.ps1'),
    read('kiminola/src-tauri/src/models.rs'),
  ]);

  assert.match(toolchain, /channel\s*=\s*"\d+\.\d+\.\d+"/);
  assert.match(toolchain, /components\s*=\s*\["rustfmt", "clippy"\]/);
  assert.match(dependabot, /package-ecosystem: "npm"/);
  assert.match(dependabot, /package-ecosystem: "cargo"/);
  assert.match(dependabot, /package-ecosystem: "github-actions"/);
  assert.match(hardware, /workflow_dispatch:/);
  assert.match(hardware, /schedule:/);
  assert.match(hardware, /group: hardware-validation-\$\{\{ inputs\.release_gate_id \|\| 'routine' \}\}/);
  assert.match(hardware, /classic_loopback_captures_non_silent_audio/);
  assert.match(hardware, /process_loopback_captures_non_silent_audio/);
  assert.match(hardware, /physical_microphone_captures_known_tone/);
  assert.match(hardware, /KIMINOLA_EXPECTED_MICROPHONE_NAME/);
  assert.match(hardware, /KIMINOLA_MICROPHONE_CAPTURE_MODE = 'baseline'/);
  assert.match(hardware, /KIMINOLA_MICROPHONE_CAPTURE_MODE = 'stimulus'/);
  assert.match(hardware, /KIMINOLA_MICROPHONE_METRICS_PATH/);
  assert.match(hardware, /baselineMetrics\.target_amplitude/);
  assert.match(hardware, /stimulusMetrics\.target_amplitude/);
  assert.match(hardware, /microphone-stimulus-ready/);
  assert.match(hardware, /\$audioSource\.Refresh\(\)/);
  assert.match(hardware, /cargo test --locked --lib --target '\$\{\{ matrix\.target \}\}' --no-run[\s\S]*?Start-Process powershell\.exe/);
  const hardwareJob = hardware.indexOf('  hardware-integration:');
  const recoveryStep = hardware.indexOf('Recover interrupted hardware state', hardwareJob);
  const nodeSetup = hardware.indexOf('Set up Node.js', hardwareJob);
  const microphoneStep = hardware.indexOf('Verify physical microphone', hardwareJob);
  assert.ok(
    recoveryStep > hardwareJob && recoveryStep < nodeSetup && recoveryStep < microphoneStep,
    'interrupted runner state is recovered before tool setup or persistent hardware fixtures are read'
  );
  assert.match(hardware, /session_transcribes_speech_wav/);
  assert.match(hardware, /production_model_pack_matches_embedded_manifest/);
  assert.match(hardware, /KIMINOLA_REQUIRE_ASR_FIXTURE/);
  assert.match(hardware, /candidateVersion/);
  assert.match(hardware, /releaseVersion -ge \$candidateVersion/);
  assert.match(hardware, /releases\?per_page=100/);
  assert.match(hardware, /\$highestEligibleTag = \$candidates\[0\]\.Release\.tag_name/);
  assert.match(hardware, /if \(\$tag -ne \$highestEligibleTag\)[\s\S]*?highest eligible baseline/);
  assert.doesNotMatch(hardware, /releases\/latest/);
  assert.match(hardware, /hardware-update-validation\.ps1/);
  assert.match(hardware, /-Target '\$\{\{ matrix\.target \}\}'/);
  assert.match(hardware, /RuntimeInformation.*ProcessArchitecture/);
  assert.match(hardware, /clang\.exe[\s\S]*?clang-cl\.exe[\s\S]*?libclang\.dll[\s\S]*?verify-pe-architecture\.ps1/);
  assert.doesNotMatch(hardware, /^  hardware-not-configured:/m);
  assert.doesNotMatch(hardware, /choco install llvm/i);
  assert.doesNotMatch(hardware, /gh release download/i);
  assert.match(nativePreparation, /Add-BuildPath -Path \$libDir/);
  assert.match(models, /microphone_tone_amplitude/);
  assert.match(models, /KIMINOLA_MICROPHONE_CAPTURE_MODE/);
  assert.match(models, /KIMINOLA_MICROPHONE_METRICS_PATH/);
  assert.match(models, /KIMINOLA_EXPECTED_MICROPHONE_NAME/);
  assert.match(models, /440\.0/);
});

test('hardware update validation preserves persistent state and exercises migration', async () => {
  const [script, recovery, stateHelpers, legacyStartup, database] = await Promise.all([
    read('kiminola/scripts/hardware-update-validation.ps1'),
    read('kiminola/scripts/hardware-recovery.ps1'),
    read('kiminola/scripts/hardware-runner-state.ps1'),
    read('kiminola/scripts/legacy-startup.test.ps1'),
    read('kiminola/src-tauri/src/db.rs'),
  ]);

  assert.match(script, /Get-FileHash/);
  assert.match(script, /model hashes changed/i);
  assert.match(script, /hardware_update_database_record_survives/);
  assert.match(script, /\.exe\.sig|minisign/);
  assert.match(script, /FromBase64String\(\(Get-Content -Raw -LiteralPath \$previousSignaturePath\)\.Trim\(\)\)/);
  assert.match(script, /-x \$rawPreviousSignaturePath/);
  assert.match(script, /Remove-Item Env:GH_TOKEN[\s\S]*?Start-Process -FilePath \$previousInstallerPath/);
  assert.match(script, /Get-Process -Name 'kiminola'[\s\S]*?New-Item -ItemType Directory -Path \$fixtureRoot/);
  const verifiedPreviousInstaller = script.indexOf('& $minisign -V -q');
  const firstDestructiveBackup = script.indexOf('Backup-HardwarePath -State $appState');
  assert.ok(
    verifiedPreviousInstaller >= 0 && verifiedPreviousInstaller < firstDestructiveBackup,
    'all previous-installer download and signature preflight must finish before persistent state moves'
  );
  assert.match(script, /try \{\s*Export-HardwareRegistryKey -State \$uninstallRegistryState/);
  assert.match(recovery, /Restore-HardwarePath -State \$state/);
  assert.match(recovery, /Restore-HardwareRegistryKey -State \$state/);
  assert.match(recovery, /GetFolderPath\('Programs'\)/);
  assert.match(recovery, /GetFolderPath\('Desktop'\)/);
  assert.match(script, /KiminolaHardwareRecovery/);
  assert.match(script, /active-transaction\.json/);
  assert.match(script, /hardware-recovery\.ps1/);
  assert.match(recovery, /Invoke-KiminolaInterruptedHardwareRecovery/);
  assert.match(script, /legacy-startup\.test\.ps1/);
  assert.match(script, /Invoke-InstalledAsrFixture/);
  assert.match(legacyStartup, /if \(-not \$process\.WaitForExit\(10000\)\)/);
  assert.match(legacyStartup, /KIMINOLA_HARDWARE_UPDATE_MODE = 'legacy-ready'/);
  assert.match(legacyStartup, /hardware_update_database_record_survives/);
  assert.match(
    legacyStartup,
    /if \(\$LASTEXITCODE -eq 0\) \{\s*\$process\.Refresh\(\)\s*if \(\$process\.HasExited\)/,
    'legacy readiness is accepted only while the historical executable is still alive'
  );
  assert.match(script, /legacy-startup\.test\.ps1'[\s\S]*?-Target \$Target[\s\S]*?-CargoManifestPath \$cargoManifestFullPath/);
  assert.match(database, /verify_previous_database_ready/);
  assert.match(script, /ExpectedMigrationCount \$expectedPreviousMigrationCount/);
  assert.ok((script.match(/Assert-InstalledArchitecture/g)?.length ?? 0) >= 3);
  for (const dll of ['onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll']) {
    assert.match(script, new RegExp(dll.replaceAll('.', '\\.')));
  }
  const previousInstall = script.indexOf('$previousInstall = Start-Process');
  const previousArchitecture = script.indexOf('Assert-InstalledArchitecture', previousInstall);
  const legacyStartupProbe = script.indexOf('legacy-startup.test.ps1', previousInstall);
  const currentInstall = script.indexOf('$currentInstall = Start-Process');
  const currentArchitecture = script.indexOf('Assert-InstalledArchitecture', currentInstall);
  const currentStartupProbe = script.indexOf('startup-behavior.test.ps1', currentInstall);
  assert.ok(previousArchitecture > previousInstall && previousArchitecture < legacyStartupProbe);
  assert.ok(currentArchitecture > currentInstall && currentArchitecture < currentStartupProbe);
  const interruptedRecovery = script.indexOf('hardware-recovery.ps1');
  const modelHashing = script.indexOf('$modelHashes = @{}');
  assert.ok(
    interruptedRecovery >= 0 && interruptedRecovery < modelHashing,
    'an interrupted persistent-state transaction is recovered before provisioned state is read'
  );
  const initialJournal = script.indexOf('Write-HardwareRecoveryJournal');
  assert.ok(
    initialJournal >= 0 && initialJournal < firstDestructiveBackup,
    'the durable transaction journal must exist before the first persistent-state move'
  );
  assert.ok(
    (script.match(/Write-HardwareRecoveryJournal/g)?.length ?? 0) >= 11,
    'the transaction journal is rewritten after every filesystem snapshot and every phase of each registry snapshot'
  );
  const statePlan = script.indexOf('Test-HardwareStatePlan');
  const firstRegistryExport = script.indexOf('Export-HardwareRegistryKey -State $uninstallRegistryState');
  const secondRegistryExport = script.indexOf('Export-HardwareRegistryKey -State $installRegistryState');
  const firstRegistryRemoval = script.indexOf('Start-HardwareRegistryRemoval -State $uninstallRegistryState');
  assert.ok(statePlan >= 0 && statePlan < initialJournal, 'the complete state plan is validated before transaction creation');
  assert.ok(
    firstRegistryExport > initialJournal && secondRegistryExport > firstRegistryExport && secondRegistryExport < firstDestructiveBackup,
    'both validated registry exports finish before the first destructive filesystem move'
  );
  assert.ok(firstRegistryRemoval > firstDestructiveBackup, 'live registry deletion starts only after non-destructive export preflight');
  assert.match(stateHelpers, /InitiallyExisted/);
  assert.match(stateHelpers, /SnapshotComplete/);
  assert.match(stateHelpers, /No complete snapshot exists/);
  assert.match(stateHelpers, /reg\.exe export/);
  assert.match(stateHelpers, /reg\.exe import/);
  assert.match(stateHelpers, /ExportComplete/);
  assert.match(stateHelpers, /RemovalStarted/);
  assert.match(stateHelpers, /LiveKeyRemoved/);
  assert.match(stateHelpers, /function Export-HardwareRegistryKey/);
  assert.match(stateHelpers, /function Test-HardwareStatePlan/);
  assert.match(stateHelpers, /FileOptions\]::WriteThrough/);
  assert.match(stateHelpers, /\.Flush\(\$true\)/);
  assert.match(stateHelpers, /MoveFileEx/);
  assert.match(recovery, /orphaned transaction director/);
  assert.match(stateHelpers, /Remove-Item -LiteralPath \$State\.RegistryPath/);
  assert.match(stateHelpers, /function Write-HardwareRecoveryJournal/);
  assert.match(stateHelpers, /restore_complete/);
  assert.match(
    recovery,
    /restore_complete[\s\S]*?if \(Test-Path -LiteralPath \$transactionRoot\)[\s\S]*?Remove-Item -LiteralPath \$transactionRoot -Recurse -Force -ErrorAction Stop[\s\S]*?Test-Path -LiteralPath \$transactionRoot/,
    'completed recovery cleanup retains its journal unless transaction data is gone',
  );
  assert.match(stateHelpers, /function Read-HardwareRecoveryJournal/);
  assert.match(stateHelpers, /function Apply-HardwareRecoveryJournal/);
  const candidateStartup = script.indexOf("startup-behavior.test.ps1') -Executable $installedExecutable", script.indexOf('$currentInstall'));
  const databaseVerify = script.indexOf('Invoke-DatabaseFixture -Mode verify');
  assert.ok(candidateStartup >= 0 && databaseVerify > candidateStartup, 'candidate starts and migrates before current-code verification');
  const postInstallAsr = script.indexOf('Invoke-InstalledAsrFixture', currentInstall);
  assert.ok(postInstallAsr > currentInstall, 'ASR is revalidated after the candidate installer runs');
  assert.match(database, /seed_hardware_update_record/);
  assert.match(database, /verify_current_migration_history/);
  assert.match(database, /SELECT version, checksum, success FROM _sqlx_migrations/);
  assert.match(database, /crate::migrations::compatible/);
  assert.match(database, /SqlitePoolOptions[\s\S]*?create_if_missing\(false\)/);
  assert.match(database, /SELECT id FROM spaces[\s\S]*?INSERT INTO meetings\(title, space_id/);
  assert.doesNotMatch(script, /hardware-update-marker\.txt/);
});

test('release documentation identifies the published updater bridge accurately', async () => {
  const [readme, releasing] = await Promise.all([read('README.md'), read('docs/RELEASING.md')]);

  assert.match(readme, /v0\.1\.2[^\n]*first updater-enabled release/i);
  assert.doesNotMatch(readme, /\bsigned `latest\.json`/i);
  assert.match(releasing, /v0\.1\.2[^\n]*first updater-enabled release/i);
  assert.match(releasing, /after publication[\s\S]{0,200}in-app updater/i);
  assert.match(releasing, /Native package jobs also run[\s\S]{0,100}startup behavior/i);
  assert.doesNotMatch(releasing, /before publishing[^\n]*accept[^\n]*update through the app/i);
  assert.match(releasing, /SIGNPATH_SIGNER_THUMBPRINT/);
  assert.match(releasing, /<zip-file>/);
  assert.match(releasing, /<pe-file path="\*\.exe"/);
});
