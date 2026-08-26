// Opening and closing documents, including the password flow.

import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

import { commands, type DocumentInfo, type OpenError } from "../ipc.js";
import { notify, resetDocumentState, state } from "../state.js";
import { invalidate } from "../viewer/viewer.js";
import { applyFit } from "./navigation.js";
import { loadOutline } from "./outline.js";

type Prompt = (message: string) => Promise<string | null>;

export async function openDocument(
  askForPassword: Prompt,
  onError: (message: string) => void,
): Promise<void> {
  // The dialog call belongs inside the try as well: when the capability grant
  // was missing it rejected outside any handler, so the click did nothing at
  // all, with no message anywhere.
  try {
    const selected = await openFileDialog({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (typeof selected !== "string") return;

    await loadDocument(selected, undefined, askForPassword, onError);
  } catch (error) {
    onError(`Could not open document: ${String(error)}`);
  }
}

/**
 * Opens a path, prompting for a password as many times as the user is willing.
 *
 * Retries rather than giving up after one wrong password: mistyping is the
 * common case, and making the user reopen the file for it is needless.
 */
async function loadDocument(
  path: string,
  password: string | undefined,
  askForPassword: Prompt,
  onError: (message: string) => void,
): Promise<void> {
  let attempt = password;
  let message = "This document is password protected.";

  for (;;) {
    try {
      const info: DocumentInfo = await commands.openDocument(path, attempt ?? null);

      state.document = info;
      resetDocumentState();
      notify("document");

      invalidate(true);
      await applyFit("fit-page");
      await loadOutline();
      return;
    } catch (error) {
      const failure = error as Partial<OpenError>;
      if (failure?.needsPassword !== true) {
        onError(`Could not open document: ${failure?.message ?? String(error)}`);
        return;
      }

      const entered = await askForPassword(message);
      if (entered === null) return; // cancelled; not an error
      attempt = entered;
      message = "That password was not accepted. Try again.";
    }
  }
}
