# Native UI previews

This small Windows harness renders the actual Account, retainer listings, notification history and
navigation components using the stable Dalamud ImGui binding. It uses a software
triangle renderer and a Windows system font; no game process, account or GPU is
needed. The plugin itself continues to use the player's Dalamud font and theme.

`build-plugin.ps1 -StageOnly` runs this alongside the managed/native checks and
writes previews to `artifacts/dalamud/ui/`. To iterate independently, provide fresh
stable references from `check-updates.ps1`:

```powershell
dotnet run --project plugins/dalamud/Ultros.Ui.Smoke -c Release -p:DALAMUD_HOME=<absolute-reference-directory>
```

The fixtures include connected, disconnected, awaiting approval, populated history
and empty history at normal and narrow widths, plus larger-font account and retainer views. Account identity and pairing codes
must remain hidden in these default views. These previews supplement live in-game
testing; they do not simulate game services or replace testing with real fonts and
font scaling in Dalamud.
