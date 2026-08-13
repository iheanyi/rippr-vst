import assert from "node:assert/strict";
import test from "node:test";

import {
  bumpVersion,
  packageVersions,
  parseVersion,
  readWorkspaceVersion,
  setWorkspaceLockVersions,
  setWorkspaceVersion,
} from "./release-version.mjs";

test("bumps stable semantic versions", () => {
  assert.equal(bumpVersion("1.2.3", "patch"), "1.2.4");
  assert.equal(bumpVersion("1.2.3", "minor"), "1.3.0");
  assert.equal(bumpVersion("1.2.3", "major"), "2.0.0");
});

test("rejects invalid versions and bump names", () => {
  assert.throws(() => parseVersion("v1.2.3"), /stable semantic version/);
  assert.throws(() => parseVersion("1.2"), /stable semantic version/);
  assert.throws(() => bumpVersion("1.2.3", "banana"), /Unknown version bump/);
});

test("updates only the workspace package version", () => {
  const cargoToml = `[workspace]\nresolver = "3"\n\n[workspace.package]\nversion = "1.2.3"\nedition = "2024"\n\n[workspace.dependencies]\nserde = "1"\n`;
  const updated = setWorkspaceVersion(cargoToml, "1.3.0");
  assert.equal(readWorkspaceVersion(updated), "1.3.0");
  assert.match(updated, /serde = "1"/);
});

test("updates every Rippr package in Cargo.lock", () => {
  const cargoLock = ["rippr-core", "rippr-plugin", "rippr-worker"]
    .map((name) => `[[package]]\nname = "${name}"\nversion = "1.2.3"\n`)
    .join("\n");
  const updated = setWorkspaceLockVersions(cargoLock, "1.2.4");
  assert.equal((updated.match(/version = "1\.2\.4"/g) ?? []).length, 3);
});

test("reads the UI manifest and lockfile versions", () => {
  const versions = packageVersions(
    JSON.stringify({ version: "1.2.3" }),
    JSON.stringify({ version: "1.2.3", packages: { "": { version: "1.2.3" } } }),
  );
  assert.deepEqual(versions, { manifest: "1.2.3", lock: "1.2.3", lockRoot: "1.2.3" });
});
