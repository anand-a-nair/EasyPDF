// Pointer tools.
//
// Text selection owned the page's pointer events outright. Phase 2 needs a
// highlight tool, an ink tool, a note tool and shape tools, all of which want
// the same events and only one of which should have them at a time.
//
// A tool is three optional handlers and a name. Registering one is all a
// feature has to do; the router below decides who gets the event.

export interface ToolEvent {
  readonly page: number;
  readonly event: PointerEvent;
  /** Pointer position in PDF page coordinates: points, origin bottom-left. */
  readonly point: { x: number; y: number } | null;
}

export interface Tool {
  readonly name: string;
  /** CSS cursor while this tool is active. */
  readonly cursor?: string;
  onPointerDown?(event: ToolEvent): void;
  onPointerMove?(event: ToolEvent): void;
  onPointerUp?(event: ToolEvent): void;
  /** Called when another tool takes over, so state can be abandoned cleanly. */
  onDeactivate?(): void;
}

const tools = new Map<string, Tool>();
let active: string = "select";

export function registerTool(tool: Tool): void {
  tools.set(tool.name, tool);
}

export function activeTool(): Tool | undefined {
  return tools.get(active);
}

export function setActiveTool(name: string): void {
  if (name === active) return;
  tools.get(active)?.onDeactivate?.();
  active = name;
}

export function activeToolName(): string {
  return active;
}

/** Routes a pointer event to the active tool. */
export function dispatch(
  kind: "down" | "move" | "up",
  event: ToolEvent,
): void {
  const tool = tools.get(active);
  if (tool === undefined) return;

  if (kind === "down") tool.onPointerDown?.(event);
  else if (kind === "move") tool.onPointerMove?.(event);
  else tool.onPointerUp?.(event);
}
