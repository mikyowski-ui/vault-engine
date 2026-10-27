tactical-camera
===============

Version: 1.0.0

Enhanced viewport, camera and roof-peek settings (with any table overrides and enhanced scripts in use), exported with cargo xtask export-mod.

A Vault Engine mod layer: a folder the engine mounts above the game data.

On launch, while the layer is enabled, its vault-mod.toml applies these
[enhanced] settings (command-line flags still win):
  preset = enhanced
  viewport_mode = unlocked_widescreen
  default_zoom = 1.25
  smooth_camera = true
  roof_peek = cursor_xray

It carries no game files: only the settings above.

To load it:
1. Put this folder beside your load_order.toml (or vault-engine.toml),
   for example in mods/.
2. Start the engine with --launcher and open the Mods tab.
3. Type the folder's path (relative to the load order, such as
   mods/<this folder>), press Add, then Save.
4. Press Play. The list shows the mod's name, and [enhanced] when it
   carries launch settings. Disable or remove the layer to stop using it.

Or add it to load_order.toml by hand:

  [[layer]]
  path = "mods/<this folder>"
