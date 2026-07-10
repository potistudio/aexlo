# Agent Guidances

## Development

MUST commit when you make any changes.

## Architecture

This crate is a **host** for existing After Effects plugin instances (`.aex` files).

All plugin parameters and internal settings are **defined and owned by the plugin instance itself**, not by the host. After initialization, these values are treated as immutable from the host's perspective.

The host must NOT call `add_instance_param` or `clear_instance_params` to manipulate parameters. Those methods exist only to bridge data received from the plugin during its own initialization sequence (e.g., `PF_Cmd_PARAMS_SETUP`). Once initialization is complete, the host reads parameters — it does not write them.
