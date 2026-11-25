using Dalamud.Bindings.ImGui;

namespace UltrosPlugin.Ui;

internal sealed record SettingsState(
    bool ChatEnabled,
    int ChatIndex,
    string[] ChatLabels,
    bool PopupEnabled,
    bool SoundEnabled,
    int SoundId,
    bool ItemContextMenuEnabled,
    bool BellShortcutEnabled,
    bool InventoryProgressEnabled,
    bool CanEnableInventory,
    bool InventoryNeedsBuildConfirm,
    string InventoryStatus,
    bool LocalhostAvailable,
    bool Localhost,
    string LocalOrigin,
    string Profile,
    string CaptureCompatibilityStatus,
    string? GameBuild);

internal sealed class SettingsPanel
{
    private string profile = "default";
    private string localOrigin = "http://localhost:8080";
    private bool localhost;
    private bool initialized;

    internal Action<bool> SetChatEnabled { get; init; } = _ => { };
    internal Action<int> SetChatIndex { get; init; } = _ => { };
    internal Action<bool> SetPopupEnabled { get; init; } = _ => { };
    internal Action<bool> SetSoundEnabled { get; init; } = _ => { };
    internal Action<int> SetSoundId { get; init; } = _ => { };
    internal Action TestNotification { get; init; } = () => { };
    internal Action<bool> SetItemContextMenu { get; init; } = _ => { };
    internal Action<bool> SetBellShortcut { get; init; } = _ => { };
    internal Action<bool> SetInventoryProgress { get; init; } = _ => { };
    internal Action ConfirmInventoryBuild { get; init; } = () => { };
    internal Action<bool, string, string> SaveConnection { get; init; } = (_, _, _) => { };
    internal Action OpenLab { get; init; } = () => { };

    internal void Draw(SettingsState s)
    {
        if (!initialized)
        {
            profile = s.Profile;
            localOrigin = s.LocalOrigin;
            localhost = s.Localhost;
            initialized = true;
        }

        ImGui.SetWindowFontScale(1.12f);
        ImGui.TextUnformatted("Alerts");
        ImGui.SetWindowFontScale(1f);

        var chatEnabled = s.ChatEnabled;
        if (ImGui.Checkbox("Print alerts in game chat", ref chatEnabled)) SetChatEnabled(chatEnabled);
        UiKit.Hint("Turn off to hide [Ultros] chat lines (for example while streaming). Alerts stay in the Notifications tab.");
        if (chatEnabled)
        {
            var chat = s.ChatIndex;
            ImGui.SetNextItemWidth(220 * UiKit.Scale);
            if (ImGui.Combo("Chat category", ref chat, s.ChatLabels, s.ChatLabels.Length)) SetChatIndex(chat);
            UiKit.Hint("Enable this category in your game chat tab's filters to see [Ultros] messages.");
        }

        var popup = s.PopupEnabled;
        var playSound = s.SoundEnabled;
        if (ImGui.Checkbox("Show popup toast", ref popup)) SetPopupEnabled(popup);
        if (ImGui.Checkbox("Play notification sound", ref playSound)) SetSoundEnabled(playSound);
        if (playSound)
        {
            var sound = s.SoundId;
            ImGui.SetNextItemWidth(220 * UiKit.Scale);
            if (ImGui.SliderInt("Sound effect", ref sound, 1, 16)) SetSoundId(sound);
        }
        if (ImGui.Button("Test notification")) TestNotification();

        UiKit.Section("In-game UI & shortcuts");
        var itemShortcuts = s.ItemContextMenuEnabled;
        if (ImGui.Checkbox("Show item right-click shortcuts", ref itemShortcuts)) SetItemContextMenu(itemShortcuts);
        UiKit.Hint("Add Ultros actions (Search Ultros, Add to list, Open website) to in-game item context menus.");

        var bellShortcut = s.BellShortcutEnabled;
        if (ImGui.Checkbox("Show summoning-bell Ultros button", ref bellShortcut)) SetBellShortcut(bellShortcut);
        UiKit.Hint("Display the quick Ultros retainer button on the summoning-bell window header. Turn off to keep the bell unmodified.");

        UiKit.Section("List progress");
        var automatic = s.InventoryProgressEnabled;
        ImGui.BeginDisabled(!s.CanEnableInventory && !automatic);
        if (ImGui.Checkbox("Track new inventory items", ref automatic)) SetInventoryProgress(automatic);
        ImGui.EndDisabled();
        UiKit.Hint("Automatically mark incoming items acquired on your active writable list.");
        if (s.InventoryNeedsBuildConfirm && UiKit.PrimaryButton("Resume inventory tracking for this build"))
            ConfirmInventoryBuild();
        UiKit.Hint(s.InventoryStatus);
        if (ImGui.CollapsingHeader("How inventory progress works"))
            ImGui.TextWrapped("New items entering your bags count toward the active writable list. Existing items are the baseline. Transfers from retainers and saddlebags count; moving items within bags and equipment does not. Consuming items does not undo progress.");

        UiKit.Section("Advanced");
        if (ImGui.CollapsingHeader("Connection profile"))
        {
            ImGui.SetNextItemWidth(240 * UiKit.Scale);
            ImGui.InputText("Game-process profile", ref profile, 32);
            if (s.LocalhostAvailable) ImGui.Checkbox("Target localhost development server", ref localhost);
            if (localhost)
            {
                ImGui.SetNextItemWidth(260 * UiKit.Scale);
                ImGui.InputText("Local origin", ref localOrigin, 128);
            }
            if (ImGui.Button("Save connection settings (reload plugin)"))
                SaveConnection(localhost, localOrigin, profile);
        }
        if (ImGui.CollapsingHeader("Diagnostics"))
        {
            ImGui.TextWrapped($"Capture compatibility: {s.CaptureCompatibilityStatus}");
            ImGui.TextWrapped($"Game build: {s.GameBuild ?? "ffxivgame.ver unavailable"}");
            UiKit.Hint("Retainer uploader controls live under Retainers → Manage uploads.");
        }
        if (s.LocalhostAvailable && ImGui.Button("Open local CRDT merge lab")) OpenLab();
    }
}
