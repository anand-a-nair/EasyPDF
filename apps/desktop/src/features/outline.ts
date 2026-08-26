// The document outline, as a sidebar panel.

import { commands } from "../ipc.js";
import { notify, state } from "../state.js";
import { registerPanel } from "../ui/panels.js";
import { goToPage } from "./navigation.js";

export function registerOutlinePanel(): void {
  let list: HTMLElement;
  let empty: HTMLElement;

  registerPanel({
    name: "outline",
    title: "Outline",

    mount(container) {
      const heading = document.createElement("h2");
      heading.textContent = "Outline";

      list = document.createElement("ul");
      list.className = "outline-list";

      empty = document.createElement("p");
      empty.className = "panel-empty";
      empty.textContent = "This document has no outline.";
      empty.hidden = true;

      container.append(heading, list, empty);
    },

    refresh() {
      list.replaceChildren();
      empty.hidden = state.outline.length > 0;

      for (const entry of state.outline) {
        const item = document.createElement("li");
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = entry.title || "(untitled)";
        // Indentation carries the nesting the flattened list would lose.
        button.style.paddingLeft = `${0.4 + entry.depth * 0.75}rem`;

        if (entry.page === null) {
          // Shown but not clickable: an entry whose action is not a page jump
          // should not silently send the user to page one.
          button.disabled = true;
          button.title = "This entry does not link to a page";
        } else {
          const target = entry.page;
          button.addEventListener("click", () => goToPage(target));
        }

        item.append(button);
        list.append(item);
      }
    },
  });
}

export async function loadOutline(): Promise<void> {
  try {
    state.outline = await commands.outline();
  } catch {
    // A document without a readable outline is common and not an error.
    state.outline = [];
  }
  notify("outline");
}
