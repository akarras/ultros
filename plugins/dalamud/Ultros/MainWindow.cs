using System.Numerics;
using Dalamud.Bindings.ImGui;
using Dalamud.Interface.Textures;
using Dalamud.Interface.Windowing;
using Ultros.Client;
using Ultros.Lists;
using UltrosPlugin.Ui;
using Item = Lumina.Excel.Sheets.Item;

namespace UltrosPlugin;

internal sealed class MainWindow : Window
{
    private readonly ClientWorker worker;
    private readonly RetainerCapture capture;
    private readonly InventoryAcquisition inventoryProgress;
    private readonly Configuration config;
    private readonly AccountPanel account;
    private readonly MarketPanel market;
    private readonly ListsPanel lists;
    private readonly NotificationPanel notifications;
    private readonly RetainerListingsPanel retainerListings;
    private Page page = Page.Account;
    private bool firstDraw = true;
    private bool localhost;
    private string localOrigin, profile;
    private bool openItem, openLists, openRetainers, manageRetainers;
    private string uiStatus = "";
    private string? browserError;

    internal MainWindow(ClientWorker worker, RetainerCapture capture, InventoryAcquisition inventoryProgress, Configuration config, GameRecipeCatalog recipeCatalog)
        : base($"Ultros v{ClientBuild.Version.ToString(4)}###UltrosConnected")
    {
        this.worker = worker;
        this.capture = capture;
        this.inventoryProgress = inventoryProgress;
        this.config = config;

        account = new AccountPanel
        {
            Connect = o => Submit(worker.Pair(o.Label, o.EditLists, o.Register, o.Upload, o.ReadRetainers)),
            Disconnect = () => Submit(worker.Disconnect()),
            CancelPairing = () => Submit(worker.CancelPairing()),
            Refresh = () => Submit(worker.Refresh()),
            Open = Open,
        };
        market = new MarketPanel
        {
            ItemName = ItemName,
            WorldName = WorldName,
            Marketable = Marketable,
            HasItem = HasItem,
            SearchItems = SearchMarketableItems,
            ItemIcon = ItemIcon,
            FetchItem = (id, s) => Submit(worker.FetchItem(id, s)),
            AddToList = (id, quality, qty) => AddToList(id, ListEditTarget.Capture(worker.View), quality, qty),
            OpenWebsite = OpenItemWebsite,
        };
        lists = new ListsPanel
        {
            ItemName = ItemName,
            WorldName = WorldName,
            HasItem = HasItem,
            SearchAllItems = SearchAllItems,
            ItemIcon = ItemIcon,
            RecipeCatalog = recipeCatalog,
            SearchRecipes = recipeCatalog.SearchRecipes,
            CanSyncBagStock = () => inventoryProgress.CanSnapshot,
            SyncBagStock = target =>
            {
                if (inventoryProgress.TrySnapshotTotals(out var totals))
                    Submit(worker.SyncBagStock(target, totals));
                else
                    uiStatus = "Waiting for a loaded character and compatible inventory reader.";
            },
            Refresh = () => Submit(worker.Refresh()),
            SelectList = id => Submit(worker.SelectList(id)),
            RetrySave = target => Submit(worker.RetrySave(target)),
            RefreshEstimate = target => Submit(worker.Estimate(target)),
            RetryRecovery = target => Submit(worker.RetryRecovery(target)),
            UseServerCopy = target => Submit(worker.UseServerCopy(target)),
            AddItem = (dest, id, quality, qty) => Submit(worker.AddItem(dest, id, quality, qty)),
            AddTarget = (dest, recipeId, count) => Submit(worker.AddTarget(dest, recipeId, count)),
            Edit = (target, edit) => Submit(worker.Edit(target, edit)),
            OpenItem = id => ShowItem(id),
            Notify = msg => uiStatus = msg,
        };
        notifications = new NotificationPanel
        {
            Test = () => Submit(worker.TestNotification()),
            Refresh = () => Submit(worker.Refresh()),
            MarkRead = id => Submit(worker.MarkRead(id)),
            Delete = (session, ids) => Submit(worker.DeleteNotifications(session, ids)),
            OpenItem = id => ShowItem(id),
            OpenList = id => { Submit(worker.SelectList(id)); openLists = true; },
        };
        retainerListings = new RetainerListingsPanel
        {
            ItemName = ItemName,
            WorldName = WorldName,
            Marketable = Marketable,
            ItemIcon = ItemIcon,
            OpenItem = id => ShowItem(id),
            SetEnabled = (id, enabled) => Submit(worker.SetRetainerEnabled(id, enabled)),
        };

        localhost = config.Localhost;
        localOrigin = config.LocalOrigin;
        profile = config.Profile;
        Size = new Vector2(960, 720);
        SizeCondition = ImGuiCond.FirstUseEver;
        SizeConstraints = new WindowSizeConstraints { MinimumSize = new Vector2(650, 440), MaximumSize = new Vector2(float.MaxValue) };
    }

    private void Submit(bool accepted) { uiStatus = accepted ? "" : "The worker queue is full; retry shortly."; }

    private void Open(string url) => _ = OpenBrowser(url);

    private async Task OpenBrowser(string url)
    {
        // Approval links are validated by Connection; other URLs are built from
        // the configured origin. Dalamud's helper logs the full URL on failure.
        if (!await BrowserLinks.Open(url).ConfigureAwait(false))
            Interlocked.Exchange(ref browserError, "Could not open your browser. Open the website manually or retry this button.");
    }

    private void OpenItemWebsite(int id, string scope)
    {
        var baseUri = new Uri(worker.View.VerificationUrl);
        var path = scope.Length > 0 ? $"/item/{Uri.EscapeDataString(scope)}/{id}" : $"/item/{id}";
        Open(new Uri(baseUri, path).AbsoluteUri);
    }

    private static bool HasItem(int id) => id > 0 && Plugin.Data.GetExcelSheet<Item>().HasRow((uint)id);
    private static string ItemName(int id) => Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)id, out var item) ? item.Name.ToString() : $"Item #{id}";
    private static bool Marketable(int id) => id > 0 && Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)id, out var item) && item.ItemSearchCategory.RowId != 0;
    private static string WorldName(int id) => id > 0 && Plugin.Data.GetExcelSheet<Lumina.Excel.Sheets.World>().TryGetRow((uint)id, out var world) ? world.Name.ToString() : $"World #{id}";

    private static (int Id, string Name)[] SearchMarketableItems(string term) =>
        Plugin.Data.GetExcelSheet<Item>()
            .Where(i => i.ItemSearchCategory.RowId != 0)
            .Select(i => (Id: checked((int)i.RowId), Name: i.Name.ToString()))
            .Where(i => i.Name.Contains(term, StringComparison.OrdinalIgnoreCase))
            .Take(25)
            .ToArray();

    private static (int Id, string Name)[] SearchAllItems(string term) =>
        Plugin.Data.GetExcelSheet<Item>()
            .Where(i => i.RowId > 0)
            .Select(i => (Id: checked((int)i.RowId), Name: i.Name.ToString()))
            .Where(i => i.Name.Length > 0 && i.Name.Contains(term, StringComparison.OrdinalIgnoreCase))
            .Take(25)
            .ToArray();

    private static ImTextureID? ItemIcon(int id, bool hq)
    {
        if (id <= 0 || !Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)id, out var item) || item.Icon == 0)
            return null;
        var lookup = new GameIconLookup(item.Icon, hq);
        return Plugin.TextureProvider.GetFromGameIcon(lookup).TryGetWrap(out var wrap, out _) ? wrap.Handle : null;
    }

    internal void ShowItem(int id, string? priceWorld = null)
    {
        market.SelectItem(id, priceWorld);
        openItem = true;
        IsOpen = true;
    }

    internal void ShowRetainers()
    {
        openRetainers = true;
        manageRetainers = true;
        IsOpen = true;
    }

    internal void AddToList(int id, ListEditTarget? destination, string quality = "any", int quantity = 1)
    {
        openItem = false;
        openLists = true;
        IsOpen = true;
        uiStatus = destination is not null && worker.AddItem(destination, id, quality, quantity)
            ? ""
            : "Choose a writable list, then add the selected item. Enable list editing in Account if needed.";
    }

    internal void AddRecipeToList(int recipeId, ListEditTarget? destination, int count = 1)
    {
        openItem = false;
        openLists = true;
        IsOpen = true;
        uiStatus = destination is not null && worker.AddTarget(destination, recipeId, count)
            ? ""
            : "Choose a writable list, then add the selected recipe. Enable list editing in Account if needed.";
    }

    public override void OnClose() => account.Reset();
    public override void OnOpen() => account.Reset();
    public override void PreDraw() => ImGui.SetNextWindowBgAlpha(1);

    public override void Draw()
    {
        if (Interlocked.Exchange(ref browserError, null) is { } browserFailure) uiStatus = browserFailure;
        var v = worker.View;
        var activatePage = firstDraw || openItem || openLists || openRetainers;
        market.SyncScope(v);
        if (firstDraw) { page = v.Connected ? Page.Market : Page.Account; firstDraw = false; }
        if (openItem) { page = Page.Market; openItem = false; }
        if (openLists) { page = Page.Lists; openLists = false; }
        if (openRetainers) { page = Page.Retainers; openRetainers = false; }

        var unread = v.Inbox.Count(d => v.ReadNotificationIds?.Contains(d.Id) != true);
        UiKit.BeginStyle();
        try
        {
            page = UiKit.Navigation(page, v.Connected, unread, activatePage);
            account.Track(v, page == Page.Account);
            UiKit.BeginContent();
            try
            {
                if (config.Localhost) UiKit.Muted("LOCALHOST DEVELOPMENT");
                switch (page)
                {
                    case Page.Account:
                        account.Draw(v);
                        break;
                    case Page.Market:
                        market.Draw(v);
                        break;
                    case Page.Lists:
                        lists.Draw(v, InventoryBuildBanner());
                        break;
                    case Page.Retainers:
                        UiKit.Heading("Retainers", "Your uploaded listings, grouped by character and retainer.");
                        Characters(v);
                        break;
                    case Page.Notifications:
                        notifications.Draw(v);
                        break;
                    case Page.Settings:
                        UiKit.Heading("Settings", "Customize alerts, optional in-game UI surfaces, and sync behavior.");
                        Settings();
                        break;
                }
                if (uiStatus.Length > 0)
                {
                    UiKit.Section("Notice");
                    ImGui.TextWrapped(uiStatus);
                    if (ImGui.SmallButton("Dismiss")) uiStatus = "";
                }
            }
            finally { UiKit.EndContent(uiStatus.Length > 0 ? uiStatus : v.Status); }
        }
        finally { UiKit.EndStyle(); }
    }

    private (string Title, string Message, string ActionLabel, Action OnAction)? InventoryBuildBanner()
    {
        if (!config.InventoryProgressEnabled || !capture.Compatible || capture.GameBuild is null || config.InventoryProgressGameBuild == capture.GameBuild)
            return null;
        return (
            "Inventory tracking paused after a game update",
            $"FFXIV updated to build {capture.GameBuild}. Confirm to resume automatic acquired-item tracking for your active list.",
            "Resume inventory tracking for this build",
            () =>
            {
                config.InventoryProgressGameBuild = capture.GameBuild;
                worker.CancelQueuedInventory();
                Plugin.Interface.SavePluginConfig(config);
                uiStatus = "Inventory progress enabled for the current game build.";
            });
    }

    private void DrawRetainerCompatibilityBanner()
    {
        if (!capture.Compatible)
        {
            UiKit.Banner("retainer-incompatible", "Plugin update needed for Dalamud client structs",
                capture.CompatibilityStatus + " Update Ultros in /xlplugins, then reload.", warning: true);
            return;
        }
        if (config.CaptureEnabled && capture.GameBuild is not null && config.CaptureGameBuild != capture.GameBuild)
        {
            UiKit.Banner("retainer-build", "Listing uploader paused after a game update",
                $"FFXIV updated to build {capture.GameBuild}. Confirm once to resume private retainer uploads.",
                "Resume listing uploader for this build",
                () =>
                {
                    config.CaptureEnabled = true;
                    config.CaptureGameBuild = capture.GameBuild;
                    Plugin.Interface.SavePluginConfig(config);
                    uiStatus = "Listing uploader enabled for the current game build. Open a retainer's Markets window to sync.";
                }, warning: true);
        }
    }

    private void Characters(ClientView v)
    {
        DrawRetainerCompatibilityBanner();
        ImGui.BeginDisabled(!v.Connected || !v.Scopes.Contains("retainers:read") || v.RefreshingRetainers);
        if (ImGui.Button(v.RefreshingRetainers ? "Refreshing…" : "Refresh listings")) Submit(worker.RefreshRetainers());
        ImGui.EndDisabled();
        UiKit.Inline(240);
        if (ImGui.Button("View listings on Ultros")) Open(new Uri(new Uri(v.VerificationUrl), "/retainers/listings").AbsoluteUri);
        UiKit.Hint(capture.Status);
        if (capture.Bell is { } bell)
        {
            var saved = ClientWorker.LatestObservation(v.Characters, bell.Character);
            ImGui.BeginDisabled(!v.RetainerPreferences || !v.Scopes.Contains("retainers:register") || saved is null || RetainerRoster.GameOrder(saved, bell.Character) is null);
            if (ImGui.Button("Use in-game order")) Submit(worker.UseGameRetainerOrder(bell.Character));
            ImGui.EndDisabled();
        }
        UiKit.Hint("Retainers follow your saved Ultros order. Use in-game order at a summoning bell to update both views.");
        retainerListings.Draw(v);
        ImGui.Spacing();
        ImGui.Spacing();
        if (manageRetainers) { ImGui.SetNextItemOpen(true, ImGuiCond.Always); manageRetainers = false; }
        if (!ImGui.CollapsingHeader("Manage uploads")) return;
        UploaderControls();
        if (v.RosterStatus.Length > 0) ImGui.TextWrapped(v.RosterStatus);
        if (capture.Character is { } current)
        {
            ImGui.TextUnformatted($"{current.Name} · {WorldName(current.HomeWorld)} · {current.Retainers.Length} retainers found");
            ImGui.BeginDisabled(!v.Connected || !v.Scopes.Contains("retainers:register") || v.Registering);
            try
            {
                if (ImGui.Button("Save character only")) Submit(worker.Observe(current));
                ImGui.SameLine();
                if (ImGui.Button(v.Registering ? "Saving retainers…" : "Register all observed retainers")) Submit(worker.Register(current));
            }
            finally { ImGui.EndDisabled(); }
        }
        foreach (var character in v.Characters)
        {
            ImGui.PushID(character.Id);
            try
            {
                UiKit.Section(character.Name);
                UiKit.Hint($"{character.Retainers.Count(r => r.Registered)} retainers saved · uploads {(character.UploadEnabled ? "allowed" : "paused")}");
                if (character.UploadEnabled && (!config.CaptureEnabled || config.CaptureGameBuild != capture.GameBuild))
                    ImGui.TextWrapped("This character allows uploads, but the uploader on this game client must be enabled above.");
                ImGui.BeginDisabled(!v.Scopes.Contains("retainers:upload") || !character.Registered);
                try
                {
                    if (ImGui.Button(character.UploadEnabled ? "Pause this character's uploads" : "Allow this character's uploads"))
                        Submit(worker.UploadConsent(character.Id, !character.UploadEnabled));
                    UiKit.Inline(220);
                    if (ImGui.Button("Delete saved listings...")) ImGui.OpenPopup("Delete saved listings?");
                    if (ImGui.BeginPopupModal("Delete saved listings?", ImGuiWindowFlags.AlwaysAutoResize))
                    {
                        ImGui.TextUnformatted($"Pause uploads and delete private snapshots for {character.Name}?");
                        if (ImGui.Button("Delete saved listings")) { Submit(worker.UploadConsent(character.Id, false, true)); ImGui.CloseCurrentPopup(); }
                        ImGui.SameLine();
                        if (ImGui.Button("Cancel")) ImGui.CloseCurrentPopup();
                        ImGui.EndPopup();
                    }
                }
                finally { ImGui.EndDisabled(); }
                retainerListings.Manage(character, v.Connected && v.RetainerPreferences && v.Scopes.Contains("retainers:register"));
            }
            finally { ImGui.PopID(); }
        }
    }

    private void UploaderControls()
    {
        var enabled = config.CaptureEnabled && capture.Compatible && capture.GameBuild is not null && config.CaptureGameBuild == capture.GameBuild;
        ImGui.TextUnformatted($"Listing sync: {(enabled ? "on" : "paused")}");
        UiKit.Hint("Selling prices you confirm in game sync privately to Ultros.");
        ImGui.BeginDisabled(!capture.Compatible || capture.GameBuild is null);
        try
        {
            if (ImGui.Button(enabled ? "Pause listing uploader" : "Enable listing uploader"))
            {
                config.CaptureEnabled = !enabled;
                config.CaptureGameBuild = capture.GameBuild;
                Plugin.Interface.SavePluginConfig(config);
                uiStatus = enabled
                    ? "Listing uploader paused on this game client."
                    : "Listing uploader enabled. Close and reopen the retainer's Markets window to capture its listings.";
            }
        }
        finally { ImGui.EndDisabled(); }
        UiKit.Hint(capture.Status);
    }

    private void Settings()
    {
        ImGui.TextUnformatted("Alerts");
        var chatEnabled = config.NotificationChatEnabled;
        if (ImGui.Checkbox("Print alerts in game chat", ref chatEnabled))
        {
            config.NotificationChatEnabled = chatEnabled;
            Plugin.Interface.SavePluginConfig(config);
        }
        UiKit.Hint("Turn off to hide [Ultros] chat lines (for example while streaming). Alerts remain available in the Notifications tab.");
        if (chatEnabled)
        {
            var chat = Math.Max(0, Array.FindIndex(GameNotifications.ChatTypes, t => (int)t == config.NotificationChatType));
            ImGui.SetNextItemWidth(220 * UiKit.Scale);
            if (ImGui.Combo("Chat category", ref chat, GameNotifications.ChatLabels, GameNotifications.ChatLabels.Length))
            {
                config.NotificationChatType = (int)GameNotifications.ChatTypes[chat];
                Plugin.Interface.SavePluginConfig(config);
            }
            UiKit.Hint("Enable this category in your game chat tab's filters to see [Ultros] messages.");
        }
        var popup = config.NotificationPopup;
        var playSound = config.NotificationSound;
        if (ImGui.Checkbox("Also show a popup notification", ref popup)) { config.NotificationPopup = popup; Plugin.Interface.SavePluginConfig(config); }
        if (ImGui.Checkbox("Play notification sound", ref playSound)) { config.NotificationSound = playSound; Plugin.Interface.SavePluginConfig(config); }
        if (playSound)
        {
            var sound = config.NotificationSoundId;
            ImGui.SetNextItemWidth(220 * UiKit.Scale);
            if (ImGui.SliderInt("Sound effect", ref sound, 1, 16)) { config.NotificationSoundId = sound; Plugin.Interface.SavePluginConfig(config); }
        }
        if (ImGui.Button("Test notification"))
        {
            Plugin.TestNotification();
            if (!config.NotificationChatEnabled && !config.NotificationPopup && !config.NotificationSound)
                uiStatus = "Game chat, popup, and sound outputs are all turned off. Enable at least one to preview alerts.";
        }

        UiKit.Section("In-game UI & shortcuts");
        var itemShortcuts = config.ItemContextMenuEnabled;
        if (ImGui.Checkbox("Show item right-click shortcuts", ref itemShortcuts))
        {
            config.ItemContextMenuEnabled = itemShortcuts;
            Plugin.Interface.SavePluginConfig(config);
        }
        UiKit.Hint("Add Ultros actions (Search Ultros, Add to list, Open website) to in-game item context menus.");

        var bellShortcut = config.BellShortcutEnabled;
        if (ImGui.Checkbox("Show summoning-bell Ultros button", ref bellShortcut))
        {
            config.BellShortcutEnabled = bellShortcut;
            Plugin.Interface.SavePluginConfig(config);
        }
        UiKit.Hint("Display the quick Ultros retainer button on the summoning-bell window header. Turn off to keep the bell window unmodified.");

        UiKit.Section("List progress");
        var automatic = config.InventoryProgressEnabled;
        var canEnable = capture.Compatible && capture.GameBuild is not null;
        ImGui.BeginDisabled(!canEnable && !automatic);
        if (ImGui.Checkbox("Track new inventory items", ref automatic))
        {
            config.InventoryProgressEnabled = automatic;
            config.InventoryProgressGameBuild = automatic ? capture.GameBuild : null;
            worker.CancelQueuedInventory();
            Plugin.Interface.SavePluginConfig(config);
        }
        ImGui.EndDisabled();
        UiKit.Hint("Mark incoming items acquired on your active list.");
        if (automatic && canEnable && config.InventoryProgressGameBuild != capture.GameBuild && ImGui.Button("Enable inventory progress for this game build"))
        {
            config.InventoryProgressGameBuild = capture.GameBuild;
            worker.CancelQueuedInventory();
            Plugin.Interface.SavePluginConfig(config);
        }
        UiKit.Hint(inventoryProgress.Status);
        if (ImGui.CollapsingHeader("How inventory progress works"))
            ImGui.TextWrapped("New items entering your bags count toward the active writable list. Existing items are the baseline. Transfers from retainers and saddlebags count; moving items within bags and equipment does not. Consuming items does not undo progress.");

        UiKit.Section("Advanced");
        if (ImGui.CollapsingHeader("Connection profile"))
        {
            ImGui.InputText("Game-process profile", ref profile, 32);
            if (UltrosEndpoint.LocalhostAvailable) ImGui.Checkbox("Target localhost development server", ref localhost);
            if (localhost) ImGui.InputText("Local origin", ref localOrigin, 128);
            if (ImGui.Button("Save connection settings (reload plugin)"))
            {
                try
                {
                    _ = new UltrosEndpoint(localhost, localOrigin);
                    if (profile.Length < 1 || profile.Any(c => !char.IsAsciiLetterOrDigit(c) && c is not '-' and not '_'))
                        throw new ArgumentException();
                    config.Localhost = localhost;
                    config.LocalOrigin = localOrigin;
                    config.Profile = profile;
                    Plugin.Interface.SavePluginConfig(config);
                    uiStatus = "Connection settings saved. Reload Ultros in Dalamud to apply.";
                }
                catch { uiStatus = "Use a loopback origin and a profile containing letters, numbers, '-' or '_'."; }
            }
        }
        if (ImGui.CollapsingHeader("Capture diagnostics"))
        {
            ImGui.TextWrapped($"Capture compatibility: {capture.CompatibilityStatus}");
            ImGui.TextWrapped($"Game build: {capture.GameBuild ?? "ffxivgame.ver unavailable"}");
            UploaderControls();
        }
        if (UltrosEndpoint.LocalhostAvailable && ImGui.Button("Open local CRDT merge lab")) Plugin.OpenLab();
    }
}
