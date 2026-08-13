#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const VERSION_PATTERN = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const WORKSPACE_PACKAGES = ["rippr-core", "rippr-plugin", "rippr-worker"];

export function parseVersion(value) {
  const match = VERSION_PATTERN.exec(value);
  if (!match) {
    throw new Error(`Expected a stable semantic version, received: ${value}`);
  }

  return match.slice(1).map(Number);
}

export function bumpVersion(version, bump) {
  const [major, minor, patch] = parseVersion(version);
  switch (bump) {
    case "major":
      return `${major + 1}.0.0`;
    case "minor":
      return `${major}.${minor + 1}.0`;
    case "patch":
      return `${major}.${minor}.${patch + 1}`;
    default:
      throw new Error(`Unknown version bump: ${bump}`);
  }
}

export function readWorkspaceVersion(cargoToml) {
  const section = cargoToml.match(/\[workspace\.package\][\s\S]*?(?=\n\[|$)/);
  const version = section?.[0].match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) {
    throw new Error("Cargo.toml is missing [workspace.package].version");
  }
  parseVersion(version);
  return version;
}

export function setWorkspaceVersion(cargoToml, version) {
  parseVersion(version);
  const sectionPattern = /(\[workspace\.package\][\s\S]*?^version\s*=\s*")[^"]+("\s*$)/m;
  if (!sectionPattern.test(cargoToml)) {
    throw new Error("Cargo.toml is missing [workspace.package].version");
  }
  return cargoToml.replace(sectionPattern, `$1${version}$2`);
}

export function setWorkspaceLockVersions(cargoLock, version) {
  parseVersion(version);
  let updated = cargoLock;
  for (const packageName of WORKSPACE_PACKAGES) {
    const escapedName = packageName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const packagePattern = new RegExp(
      `(\\[\\[package\\]\\]\\nname = "${escapedName}"\\nversion = ")[^"]+("\\n)`,
    );
    if (!packagePattern.test(updated)) {
      throw new Error(`Cargo.lock is missing the ${packageName} workspace package`);
    }
    updated = updated.replace(packagePattern, `$1${version}$2`);
  }
  return updated;
}

export function packageVersions(packageJson, packageLock) {
  const manifest = JSON.parse(packageJson);
  const lock = JSON.parse(packageLock);
  return {
    manifest: manifest.version,
    lock: lock.version,
    lockRoot: lock.packages?.[""]?.version,
  };
}

function setJsonVersion(contents, version, includeRootPackage = false) {
  parseVersion(version);
  const json = JSON.parse(contents);
  json.version = version;
  if (includeRootPackage) {
    if (!json.packages?.[""]) {
      throw new Error("package-lock.json is missing packages[\"\"]");
    }
    json.packages[""].version = version;
  }
  return `${JSON.stringify(json, null, 2)}\n`;
}

function readPaths(repoRoot) {
  return {
    cargoToml: path.join(repoRoot, "Cargo.toml"),
    cargoLock: path.join(repoRoot, "Cargo.lock"),
    packageJson: path.join(repoRoot, "ui", "package.json"),
    packageLock: path.join(repoRoot, "ui", "package-lock.json"),
  };
}

function readVersions(paths) {
  const cargoToml = fs.readFileSync(paths.cargoToml, "utf8");
  const packageJson = fs.readFileSync(paths.packageJson, "utf8");
  const packageLock = fs.readFileSync(paths.packageLock, "utf8");
  return {
    workspace: readWorkspaceVersion(cargoToml),
    ...packageVersions(packageJson, packageLock),
  };
}

function assertVersionsMatch(paths) {
  const versions = readVersions(paths);
  const mismatches = Object.entries(versions).filter(([, version]) => version !== versions.workspace);
  if (mismatches.length > 0) {
    throw new Error(
      `Version mismatch: workspace=${versions.workspace}, ${mismatches
        .map(([name, version]) => `${name}=${version}`)
        .join(", ")}`,
    );
  }

  const cargoLock = fs.readFileSync(paths.cargoLock, "utf8");
  for (const packageName of WORKSPACE_PACKAGES) {
    const expected = `name = "${packageName}"\nversion = "${versions.workspace}"`;
    if (!cargoLock.includes(expected)) {
      throw new Error(`Cargo.lock does not contain ${packageName} ${versions.workspace}`);
    }
  }
  return versions.workspace;
}

function parseArguments(argv) {
  const args = { check: false, bump: undefined, set: undefined, output: undefined };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--check") {
      args.check = true;
    } else if (["--bump", "--set", "--output"].includes(argument)) {
      const value = argv[index + 1];
      if (!value) {
        throw new Error(`${argument} requires a value`);
      }
      args[argument.slice(2)] = value;
      index += 1;
    } else {
      throw new Error(`Unknown argument: ${argument}`);
    }
  }
  return args;
}

export function run(argv, repoRoot) {
  const args = parseArguments(argv);
  const paths = readPaths(repoRoot);
  const currentVersion = assertVersionsMatch(paths);

  if (args.check) {
    if (args.bump || args.set || args.output) {
      throw new Error("--check cannot be combined with version-changing arguments");
    }
    process.stdout.write(`${currentVersion}\n`);
    return currentVersion;
  }

  if (Boolean(args.bump) === Boolean(args.set)) {
    throw new Error("Choose exactly one of --bump <major|minor|patch> or --set <version>");
  }

  const nextVersion = args.bump ? bumpVersion(currentVersion, args.bump) : args.set;
  parseVersion(nextVersion);

  fs.writeFileSync(
    paths.cargoToml,
    setWorkspaceVersion(fs.readFileSync(paths.cargoToml, "utf8"), nextVersion),
  );
  fs.writeFileSync(
    paths.cargoLock,
    setWorkspaceLockVersions(fs.readFileSync(paths.cargoLock, "utf8"), nextVersion),
  );
  fs.writeFileSync(
    paths.packageJson,
    setJsonVersion(fs.readFileSync(paths.packageJson, "utf8"), nextVersion),
  );
  fs.writeFileSync(
    paths.packageLock,
    setJsonVersion(fs.readFileSync(paths.packageLock, "utf8"), nextVersion, true),
  );

  assertVersionsMatch(paths);
  const output = `version=${nextVersion}\ntag=v${nextVersion}\n`;
  if (args.output) {
    fs.appendFileSync(args.output, output);
  }
  process.stdout.write(`${nextVersion}\n`);
  return nextVersion;
}

const scriptPath = fileURLToPath(import.meta.url);
if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try {
    const repoRoot = path.resolve(path.dirname(scriptPath), "..");
    run(process.argv.slice(2), repoRoot);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
