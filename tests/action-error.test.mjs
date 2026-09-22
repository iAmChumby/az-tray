import assert from "node:assert/strict";
import { test } from "node:test";
import { actionErrorMessage } from "../src/lib/actionError.ts";

test("shows a Tauri string rejection instead of the generic fallback", () => {
  assert.equal(actionErrorMessage("could not start azurite-blob.cmd: Access is denied", "Unable to start Blob"), "could not start azurite-blob.cmd: Access is denied");
});

test("keeps Error messages and uses the fallback for an empty rejection", () => {
  assert.equal(actionErrorMessage(new Error("port 10000 is in use"), "Unable to start Blob"), "port 10000 is in use");
  assert.equal(actionErrorMessage(" ", "Unable to start Blob"), "Unable to start Blob");
});
