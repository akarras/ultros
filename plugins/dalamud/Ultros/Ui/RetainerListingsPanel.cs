using Dalamud.Bindings.ImGui;
using Ultros.Client;

namespace UltrosPlugin.Ui;

internal sealed record RetainerChrome(
    bool UploaderOn,
    bool CaptureCompatible,
    string? GameBuild,
    string CaptureStatus,
    ObservedCharacter? ObservedCharacter,
    ObservedCharacter? BellCharacter,
    (string Title, string Message, string? ActionLabel, Action? OnAction)? Banner,
    bool ExpandManage,
    Action Refresh,
    Action OpenWebsite,
    Action<ObservedCharacter> UseGameOrder,
    Action ToggleUploader,
    Action<ObservedCharacter> RegisterAll,
    Action<string, bool, bool> UploadConsent);

/// <summary>One stable listing view for all retainers; no per-retainer lookup window.</summary>
internal sealed class RetainerListingsPanel
{
    private bool showDisabled;
    private string filter = "";

    internal Func<int, string> ItemName { get; init; } = id => $"Item #{id}";
    internal Func<int, string> WorldName { get; init; } = id => $"World #{id}";
    internal Func<int, bool> Marketable { get; init; } = _ => false;
    internal Func<int, bool, ImTextureID?>? ItemIcon { get; init; }
    internal Action<int> OpenItem { get; init; } = _ => { };
    internal Action<string, bool> SetEnabled { get; init; } = (_, _) => { };

    internal void Draw(ClientView view, RetainerChrome? chrome = null)
    {
        if (chrome?.Banner is { } banner)
            UiKit.Banner("retainer-banner", banner.Title, banner.Message, banner.ActionLabel, banner.OnAction, warning: true);

        if (chrome is not null)
        {
            ImGui.BeginDisabled(!view.Connected || !view.Scopes.Contains("retainers:read") || view.RefreshingRetainers);
            if (ImGui.Button(view.RefreshingRetainers ? "Refreshing…" : "Refresh##retainers")) chrome.Refresh();
            ImGui.EndDisabled();
            ImGui.SameLine();
            if (ImGui.Button("Open on Ultros")) chrome.OpenWebsite();

            UiKit.Inline(110);
            UiKit.Pill(chrome.UploaderOn ? "SYNC ON" : "SYNC PAUSED", chrome.UploaderOn ? UiKit.Success : UiKit.Warning);
            if (ImGui.IsItemHovered()) ImGui.SetTooltip(chrome.CaptureStatus);

            if (chrome.BellCharacter is { } bell)
            {
                var saved = ClientWorker.LatestObservation(view.Characters, bell);
                var canOrder = view.RetainerPreferences && view.Scopes.Contains("retainers:register") && saved is not null && RetainerRoster.GameOrder(saved, bell) is not null;
                UiKit.Inline(150);
                ImGui.BeginDisabled(!canOrder);
                if (ImGui.Button("Use in-game order")) chrome.UseGameOrder(bell);
                ImGui.EndDisabled();
                if (ImGui.IsItemHovered()) ImGui.SetTooltip("Save your summoning-bell retainer order to Ultros and this client.");
            }
        }

        if (!view.Connected) { UiKit.Empty("Connect to see your listings", "Open Account to connect this game client to Ultros."); return; }
        if (!view.Scopes.Contains("retainers:read")) { UiKit.Empty("Retainer access is not approved", "Enable View retainer listings in Account and approve the updated connection."); return; }
        if (view.Characters.Length == 0) { UiKit.Empty("No retainers saved yet", "Visit a summoning bell. Your characters and retainers sync automatically."); }
        else
        {
            DrawSummary(view);
            DrawFilterBar(view);
            DrawCharacterListings(view);
        }

        if (chrome is not null) DrawManageUploads(view, chrome);
    }

    private void DrawFilterBar(ClientView view)
    {
        var hasDisabled = view.Characters.SelectMany(c => c.Retainers).Any(r => r.Disabled);
        var avail = ImGui.GetContentRegionAvail().X;
        ImGui.SetNextItemWidth(Math.Max(180 * UiKit.Scale, Math.Min(320 * UiKit.Scale, avail - (hasDisabled ? 190 * UiKit.Scale : 0))));
        ImGui.InputTextWithHint("##retainer-filter", "Filter retainers or items…", ref filter, 80);
        if (hasDisabled)
        {
            UiKit.Inline(180);
            ImGui.Checkbox("Show disabled retainers", ref showDisabled);
        }
    }

    private void DrawCharacterListings(ClientView view)
    {
        var term = filter.Trim();
        foreach (var character in view.Characters)
        {
            var visible = character.Retainers.Where(r => showDisabled || !r.Disabled).ToArray();
            if (visible.Length == 0) continue;
            ImGui.PushID(character.Id);
            try
            {
                UiKit.Section($"{character.Name} · {WorldName(character.HomeWorld)}");
                if (!character.UploadEnabled) ImGui.TextColored(UiKit.Warning, "Uploads paused for this character.");
                foreach (var retainer in visible)
                {
                    var snapshot = view.RetainerSnapshots?.FirstOrDefault(s => s.RetainerId == retainer.Id);
                    var retainerMatches = term.Length == 0 || retainer.Name.Contains(term, StringComparison.OrdinalIgnoreCase);
                    var filteredListings = snapshot is null ? [] :
                        retainerMatches
                            ? snapshot.Listings
                            : snapshot.Listings.Where(l => ItemName(l.ItemId).Contains(term, StringComparison.OrdinalIgnoreCase)).ToArray();

                    if (term.Length > 0 && !retainerMatches && filteredListings.Length == 0) continue;

                    ImGui.PushID(retainer.Id);
                    try
                    {
                        var headerMeta = BuildRetainerHeader(retainer, snapshot);
                        if (ImGui.CollapsingHeader($"{headerMeta}###retainer-hdr", ImGuiTreeNodeFlags.DefaultOpen))
                        {
                            if (retainer.Disabled) UiKit.Hint("Disabled · saved listings kept; uploads paused.");
                            else if (retainer.Available == false) UiKit.Hint("Unavailable in game at the last bell visit. Disable it in Manage uploads if you no longer use it.");
                            if (retainer.GroupingConflict) { UiKit.Hint("Saved under another character. Review Manage uploads below."); continue; }
                            if (!retainer.Registered) { UiKit.Hint("Waiting for this retainer to sync with your account."); continue; }
                            if (snapshot is null)
                            {
                                UiKit.Hint(view.RetainerSnapshots is null
                                    ? (view.RefreshingRetainers ? "Loading uploaded listings…" : "Refresh listings to load this retainer's last upload.")
                                    : "Open this retainer's Markets window to upload its listings.");
                                continue;
                            }
                            if (filteredListings.Length == 0)
                            {
                                ImGui.TextUnformatted(term.Length > 0 ? "No items match your filter." : "No items for sale at the last upload.");
                                continue;
                            }
                            Listings(filteredListings);
                        }
                    }
                    finally { ImGui.PopID(); }
                }
            }
            finally { ImGui.PopID(); }
        }
    }

    private static string BuildRetainerHeader(PrivateRetainer retainer, RetainerSnapshot? snapshot)
    {
        var safeName = retainer.Name.Replace('#', '＃');
        if (snapshot is null) return safeName;
        long total = 0;
        foreach (var l in snapshot.Listings) total += l.UnitPrice * l.Quantity;
        return $"{safeName} · {snapshot.Listings.Length}/20 · {total:N0} gil · {TimestampFormat.FormatRelativeOrLocal(snapshot.ObservedAt)}";
    }

    private void DrawManageUploads(ClientView view, RetainerChrome chrome)
    {
        ImGui.Spacing();
        if (chrome.ExpandManage) ImGui.SetNextItemOpen(true, ImGuiCond.Always);
        if (!ImGui.CollapsingHeader("Manage uploads")) return;

        ImGui.TextUnformatted($"Listing sync: {(chrome.UploaderOn ? "on" : "paused")}");
        UiKit.Hint("Selling prices you confirm in game sync privately to Ultros.");
        ImGui.BeginDisabled(!chrome.CaptureCompatible || chrome.GameBuild is null);
        try
        {
            if (ImGui.Button(chrome.UploaderOn ? "Pause listing uploader" : "Enable listing uploader"))
                chrome.ToggleUploader();
        }
        finally { ImGui.EndDisabled(); }
        UiKit.Hint(chrome.CaptureStatus);

        if (view.RosterStatus.Length > 0) ImGui.TextWrapped(view.RosterStatus);
        if (chrome.ObservedCharacter is { } current)
        {
            ImGui.Spacing();
            ImGui.TextUnformatted($"{current.Name} · {WorldName(current.HomeWorld)} · {current.Retainers.Length} retainers found");
            ImGui.BeginDisabled(!view.Connected || !view.Scopes.Contains("retainers:register") || view.Registering);
            try
            {
                if (UiKit.PrimaryButton(view.Registering ? "Saving retainers…" : "Sync observed retainers"))
                    chrome.RegisterAll(current);
            }
            finally { ImGui.EndDisabled(); }
        }

        foreach (var character in view.Characters)
        {
            ImGui.PushID($"mgr-{character.Id}");
            try
            {
                UiKit.Section(character.Name);
                UiKit.Hint($"{character.Retainers.Count(r => r.Registered)} retainers saved · uploads {(character.UploadEnabled ? "allowed" : "paused")}");
                if (character.UploadEnabled && !chrome.UploaderOn)
                    ImGui.TextColored(UiKit.Warning, "This character allows uploads, but the uploader on this game client is paused above.");
                ImGui.BeginDisabled(!view.Scopes.Contains("retainers:upload") || !character.Registered);
                try
                {
                    if (ImGui.Button(character.UploadEnabled ? "Pause character uploads" : "Allow character uploads"))
                        chrome.UploadConsent(character.Id, !character.UploadEnabled, false);
                    UiKit.Inline(200);
                    if (UiKit.DangerButton("Delete saved listings...")) ImGui.OpenPopup("Delete saved listings?");
                    if (ImGui.BeginPopupModal("Delete saved listings?", ImGuiWindowFlags.AlwaysAutoResize))
                    {
                        ImGui.TextUnformatted($"Pause uploads and delete private snapshots for {character.Name}?");
                        if (UiKit.DangerButton("Delete saved listings")) { chrome.UploadConsent(character.Id, false, true); ImGui.CloseCurrentPopup(); }
                        ImGui.SameLine();
                        if (ImGui.Button("Cancel")) ImGui.CloseCurrentPopup();
                        ImGui.EndPopup();
                    }
                }
                finally { ImGui.EndDisabled(); }
                Manage(character, view.Connected && view.RetainerPreferences && view.Scopes.Contains("retainers:register"));
            }
            finally { ImGui.PopID(); }
        }
    }

    private static void DrawSummary(ClientView view)
    {
        var activeRetainers = view.Characters
            .SelectMany(c => c.Retainers)
            .Where(r => r.Registered && !r.Disabled && !r.GroupingConflict)
            .ToArray();
        if (activeRetainers.Length == 0) return;

        var activeIds = activeRetainers.Select(r => r.Id).ToHashSet();
        var snapshots = (view.RetainerSnapshots ?? []).Where(s => activeIds.Contains(s.RetainerId)).ToArray();
        var listedSlots = snapshots.Sum(s => s.Listings.Length);
        long totalGil = 0;
        foreach (var s in snapshots)
            foreach (var row in s.Listings) totalGil += row.UnitPrice * row.Quantity;

        UiKit.KpiRow(
            new KpiCard("Active retainers", activeRetainers.Length.ToString("N0"), $"{snapshots.Length} with snapshots"),
            new KpiCard("Listing slots", $"{listedSlots:N0} / {activeRetainers.Length * 20:N0}", "Across active retainers"),
            new KpiCard("Listed value", $"{totalGil:N0} gil", "Private snapshot total", UiKit.Accent));
    }

    internal void Manage(UnverifiedCharacter character, bool canManage)
    {
        UiKit.Hint("Disable retainers you no longer use. Listings and upload data are kept so you can enable them again.");
        foreach (var retainer in character.Retainers)
        {
            ImGui.PushID(retainer.Id);
            try
            {
                ImGui.BeginDisabled(!canManage || !retainer.Registered || retainer.GroupingConflict);
                var enabled = !retainer.Disabled;
                if (ImGui.Checkbox($"{retainer.Name.Replace('#', '＃')}##enabled", ref enabled)) SetEnabled(retainer.Id, enabled);
                ImGui.EndDisabled();
                if (retainer.Available == false) { ImGui.SameLine(); UiKit.Muted("Unavailable in game"); }
            }
            finally { ImGui.PopID(); }
        }
    }

    private void Item(PrivateListing row)
    {
        UiKit.ItemIcon(ItemIcon, row.ItemId, row.Hq, 20f);
        var name = ItemName(row.ItemId).Replace('#', '＃');
        if (Marketable(row.ItemId))
        {
            if (ImGui.Selectable($"{name}##item")) OpenItem(row.ItemId);
        }
        else ImGui.TextWrapped(name);
        if (ImGui.IsItemHovered()) ImGui.SetTooltip(ItemName(row.ItemId));
    }

    private void Listings(PrivateListing[] rows)
    {
        if (ImGui.GetContentRegionAvail().X < 620 * UiKit.Scale)
        {
            foreach (var row in rows)
            {
                ImGui.PushID(row.Slot);
                try
                {
                    Item(row);
                    ImGui.TextUnformatted($"{(row.Hq ? "HQ" : "NQ")} · {row.Quantity:N0} × {row.UnitPrice:N0} gil");
                    UiKit.Hint($"Total {(row.UnitPrice * row.Quantity):N0} gil");
                    ImGui.Separator();
                }
                finally { ImGui.PopID(); }
            }
            return;
        }
        if (!ImGui.BeginTable("listings", 5, ImGuiTableFlags.RowBg | ImGuiTableFlags.BordersInnerH | ImGuiTableFlags.SizingStretchProp)) return;
        try
        {
            ImGui.TableSetupColumn("Item", ImGuiTableColumnFlags.WidthStretch, 1);
            ImGui.TableSetupColumn("Qual", ImGuiTableColumnFlags.WidthFixed, 44 * UiKit.Scale);
            ImGui.TableSetupColumn("Unit price", ImGuiTableColumnFlags.WidthFixed, 94 * UiKit.Scale);
            ImGui.TableSetupColumn("Qty", ImGuiTableColumnFlags.WidthFixed, 42 * UiKit.Scale);
            ImGui.TableSetupColumn("Total", ImGuiTableColumnFlags.WidthFixed, 102 * UiKit.Scale);
            ImGui.TableHeadersRow();
            foreach (var row in rows)
            {
                ImGui.PushID(row.Slot);
                try
                {
                    ImGui.TableNextRow();
                    ImGui.TableNextColumn(); Item(row);
                    ImGui.TableNextColumn(); UiKit.QualityBadge(row.Hq);
                    ImGui.TableNextColumn(); ImGui.TextUnformatted(row.UnitPrice.ToString("N0"));
                    ImGui.TableNextColumn(); ImGui.TextUnformatted(row.Quantity.ToString("N0"));
                    ImGui.TableNextColumn(); UiKit.Muted((row.UnitPrice * row.Quantity).ToString("N0"));
                }
                finally { ImGui.PopID(); }
            }
        }
        finally { ImGui.EndTable(); }
    }
}
