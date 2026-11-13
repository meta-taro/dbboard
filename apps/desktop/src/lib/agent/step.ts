// One step of the agent's own cursor (ADR-0177). Mirrors
// `dbboard_config::ScreenStep`; the MCP server has already checked it.
export type ScreenStep =
  | { action: 'point'; target: string; message?: string; seconds?: number }
  | { action: 'click'; target: string; message?: string; seconds?: number }
  | { action: 'type'; target: string; value: string; message?: string; seconds?: number }
  | { action: 'go'; hash: string; message?: string; seconds?: number }
  | { action: 'say'; message: string; seconds?: number };
