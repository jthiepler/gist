import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const projectDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packagePath = path.join(projectDir, "package.json");
const packageLockPath = path.join(projectDir, "package-lock.json");
const cargoTomlPath = path.join(projectDir, "src-tauri", "Cargo.toml");
const cargoLockPath = path.join(projectDir, "src-tauri", "Cargo.lock");
const tauriConfigPath = path.join(projectDir, "src-tauri", "tauri.conf.json");

const [command, requestedVersion] = process.argv.slice(2);
const versionPattern = /^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function writeJson(filePath, value) {
  fs.writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`);
}

function replaceOnce(source, pattern, replacement, label) {
  if (!pattern.test(source)) {
    throw new Error(`Could not find ${label}.`);
  }
  return source.replace(pattern, replacement);
}

function readVersions() {
  const packageJson = readJson(packagePath);
  const packageLock = readJson(packageLockPath);
  const tauriConfig = readJson(tauriConfigPath);
  const cargoToml = fs.readFileSync(cargoTomlPath, "utf8");
  const cargoLock = fs.readFileSync(cargoLockPath, "utf8");
  const cargoVersion = cargoToml.match(/^version = "([^"]+)"/m)?.[1];
  const cargoLockVersion = cargoLock.match(/\[\[package\]\]\nname = "gist"\nversion = "([^"]+)"/)?.[1];

  return {
    "package.json": packageJson.version,
    "package-lock.json": packageLock.packages?.[""].version,
    "src-tauri/Cargo.toml": cargoVersion,
    "src-tauri/Cargo.lock": cargoLockVersion,
    "src-tauri/tauri.conf.json": tauriConfig.version,
  };
}

function checkVersions() {
  const versions = readVersions();
  const uniqueVersions = new Set(Object.values(versions));
  const missing = Object.entries(versions).filter(([, version]) => !version);

  if (missing.length > 0 || uniqueVersions.size !== 1) {
    console.error("Application versions are not synchronized:");
    for (const [file, version] of Object.entries(versions)) {
      console.error(`  ${file}: ${version ?? "missing"}`);
    }
    process.exitCode = 1;
    return false;
  }

  console.log(`Application version: ${[...uniqueVersions][0]}`);
  return true;
}

function setVersion(version) {
  if (!version || !versionPattern.test(version)) {
    throw new Error(`Invalid version "${version ?? ""}". Use a semantic version such as 0.1.1.`);
  }

  const packageJson = readJson(packagePath);
  packageJson.version = version;
  writeJson(packagePath, packageJson);

  const packageLockText = fs.readFileSync(packageLockPath, "utf8");
  const packageLockPattern = /("name":\s*"gist",\s*"version":\s*")[^"]+("\s*[,}])/g;
  const packageLockMatches = packageLockText.match(packageLockPattern) ?? [];
  if (packageLockMatches.length !== 2) {
    throw new Error(`Expected two root gist versions in package-lock.json, found ${packageLockMatches.length}.`);
  }
  const updatedPackageLock = packageLockText.replace(packageLockPattern, `$1${version}$2`);
  fs.writeFileSync(packageLockPath, updatedPackageLock);

  const cargoToml = fs.readFileSync(cargoTomlPath, "utf8");
  const updatedCargoToml = replaceOnce(
    cargoToml,
    /^version = "[^"]+"/m,
    `version = "${version}"`,
    "the Cargo package version",
  );
  fs.writeFileSync(cargoTomlPath, updatedCargoToml);

  const cargoLock = fs.readFileSync(cargoLockPath, "utf8");
  const updatedCargoLock = replaceOnce(
    cargoLock,
    /(\[\[package\]\]\nname = "gist"\nversion = ")[^"\n]+/,
    `$1${version}`,
    "the gist package version in Cargo.lock",
  );
  fs.writeFileSync(cargoLockPath, updatedCargoLock);

  const tauriConfig = readJson(tauriConfigPath);
  tauriConfig.version = version;
  writeJson(tauriConfigPath, tauriConfig);

  console.log(`Set application version to ${version}.`);
  checkVersions();
}

if (command === "check") {
  checkVersions();
} else if (command === "set") {
  setVersion(requestedVersion);
} else {
  console.error("Usage: node scripts/version.mjs check | set <version>");
  process.exitCode = 1;
}
