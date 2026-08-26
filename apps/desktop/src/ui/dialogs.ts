// Modal dialogs.

/**
 * Asks for a document password.
 *
 * Settles from the button handlers, not from the dialog's `close` event. Two
 * engine behaviours broke the event-driven version: `<form method="dialog">`
 * had not set `returnValue` by the time the handler ran, and `dialog.close()`
 * closed the dialog **without dispatching `close` at all** — so the promise
 * never settled and the retry loop waited forever, showing nothing.
 *
 * The password is used once, cleared from the input on every settle, and never
 * logged.
 */
export function askForPassword(message: string): Promise<string | null> {
  const dialog = document.getElementById("password-dialog") as HTMLDialogElement | null;
  const input = document.getElementById("password-input") as HTMLInputElement | null;
  const submit = document.getElementById("password-submit");
  const cancel = document.getElementById("password-cancel");
  const messageElement = document.getElementById("password-message");

  if (dialog === null || input === null || submit === null || cancel === null) {
    return Promise.resolve(null);
  }

  if (messageElement !== null) messageElement.textContent = message;
  input.value = "";

  return new Promise((resolve) => {
    let settled = false;

    const settle = (value: string | null): void => {
      if (settled) return;
      settled = true;

      submit.removeEventListener("click", onSubmit);
      cancel.removeEventListener("click", onCancel);
      input.removeEventListener("keydown", onKey);
      dialog.removeEventListener("close", onDismiss);

      input.value = "";
      resolve(value);
    };

    function onSubmit(): void {
      settle(input!.value);
      dialog!.close();
    }
    function onCancel(): void {
      settle(null);
      dialog!.close();
    }
    function onKey(event: KeyboardEvent): void {
      if (event.key === "Enter") {
        event.preventDefault();
        onSubmit();
      }
    }
    /** Escape dismisses natively, with no click to observe. */
    function onDismiss(): void {
      settle(null);
    }

    submit.addEventListener("click", onSubmit);
    cancel.addEventListener("click", onCancel);
    input.addEventListener("keydown", onKey);
    dialog.addEventListener("close", onDismiss);

    dialog.showModal();
    input.focus();
  });
}
