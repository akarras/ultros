using System.Drawing.Imaging;
using System.Numerics;
using System.Runtime.InteropServices;
using Dalamud.Bindings.ImGui;
using Ultros.Client;
using Ultros.Lists;
using UltrosPlugin.Ui;

// Render the shipped native components using Dalamud's actual ImGui binding.
// No game process, credentials or additional rendering libraries are required.
var output = args.FirstOrDefault() ?? Path.Combine("artifacts", "dalamud", "ui");
Directory.CreateDirectory(output);
var now = DateTimeOffset.UtcNow;
string[] scopes = ["lists:read", "lists:write", "retainers:read", "retainers:register", "retainers:upload", "notifications:read", "notifications:ack", "notifications:mark-read"];
var session = new DeviceSession("preview-device", "My game client", "123456789012345678", "chewbert", scopes, now, now, now.AddDays(1), 1);
var deliveries = new[]
{
    new GameDelivery("1", null, "test", "Ultros test notification", "This is your connected game client.", new("none", null, null), now.AddMinutes(5)),
    new GameDelivery("2", 24, "alert", "Price alert: Grade 8 Dark Matter", "The price on Gilgamesh is now 280 gil. Your target is 300 gil.", new("open_item", 10386, null), now.AddHours(1)),
};
var receivedTimes = new Dictionary<string, DateTimeOffset>
{
    ["1"] = now.AddMinutes(-12),
    ["2"] = now.AddMinutes(-2),
};
var marketView = new MarketView(5056, "Aether",
[
    new(63, "Copper Finch", false, 5, 145000),
    new(73, "Silver Wren", true, 1, 188330),
    new(63, "Iron Crow", true, 2, 192000),
], "Fetched just now", now);
var sales = new[]
{
    new SaleRow(true, 1, 189000, now.AddMinutes(-18).UtcDateTime.ToString("O"), 63),
    new SaleRow(false, 2, 142000, now.AddHours(-2).UtcDateTime.ToString("O"), 73),
};
var cartView = new CartView("Raid Prep Cart", "peer-1", true, false,
[
    new(8, "nq", 10, 4, null),
    new(5056, "hq", 2, 0, null),
    new(10386, "any", 5, 5, null),
]) { Version = "v1", Connected = true, Ready = true };
var estimate = new CartEstimate("v1", "Aether", now, 377260, false, 0,
[
    new(8, false, 6, 6, 600, "Priced", 100, [new(63, 100, false, 6)]),
    new(5056, true, 2, 2, 376660, "Priced", 188330, [new(73, 188330, true, 2)]),
    new(10386, null, 0, 0, 0, "Acquired", null, []),
],
[
    new(73, 376660, 2, 1),
    new(63, 600, 6, 1),
]);
var lists = new[]
{
    new ListAccess(new(1, 123, "Raid Prep Cart", default), "Owner", null),
    new ListAccess(new(2, 123, "Raid Gear Craft Plan", default), "Owner", null),
};
var view = new ClientView(true, "Connected.", "Credentials protected by Windows.", null, "https://ultros.app/plugin/verify", session,
    lists, 1, cartView, true, [], deliveries, marketView, sales, estimate, scopes,
    PriceRegion: "Aether", NotificationTransport: "Live alerts", ReadNotificationIds: ["1"],
    CartInstance: Guid.NewGuid(), ListTransport: "Live updates", NotificationReceivedAt: receivedTimes);

var craftCartView = new CartView("Raid Gear Craft Plan", "peer-craft", true, false,
[
    new(9201, "any", 0, 4, null),  // Claro Walnut Log (acquired -> Claro Walnut Thread is Ready)
    new(9202, "any", 0, 2, null),  // Ra'Kaznar Ore (2/6 acquired -> Ra'Kaznar Ingot Needs mats, rest to buy)
    new(9203, "any", 0, 4, null),  // Cobalt Ore (4/4 acquired for Silver Thread)
])
{
    Version = "v2-craft",
    Connected = true,
    Ready = true,
    ExcludeCrystals = true,
    Targets =
    [
        new(1001, 1, 0), // Courtly Lover's Shoes of Healing (WVR 100)
        new(1002, 2, 1), // Courtly Lover's Earring of Healing (GSM 100), 1/2 made
    ],
    CraftChoices =
    [
        new(9104, "craft", 2004, 0, 2), // Silver Thread (2/2 crafts DONE -> consumes 4 Cobalt Ore)
        new(9101, "craft", 2001, 0, 0), // Claro Walnut Thread (READY)
        new(9102, "craft", 2002, 1, 1), // Ra'Kaznar Ingot (1 on hand + 1 crafted, needs 4 ore more -> NEED 1 MAT; Used by both targets!)
        new(9103, "buy", 2003, 0, 0),   // Black Star (switched to BUYING)
    ],
};
var craftEstimate = new CartEstimate("v2-craft", "Aether", now, 118400, false, 0,
[
    new(9202, null, 4, 4, 4800, "Priced", 1200, [new(63, 1200, false, 4)]),
    new(9103, null, 2, 2, 113600, "Priced", 56800, [new(73, 56800, false, 2)]),
],
[
    new(73, 113600, 2, 1),
    new(63, 4800, 4, 1),
]);
var craftView = view with
{
    SelectedList = 2,
    Cart = craftCartView,
    Estimate = craftEstimate,
};

var retainerView = view with
{
    Characters = [new("1", "Mira Example", 63, true, false, true, 1, now, [new("1", "Copper Finch", 1, true, 1, now, false, 1)])],
    RetainerSnapshots = [new("1", 1, now, now, [new(0, 8, false, 99, 45), new(1, 5056, true, 1, 188330)])],
    RetainerPreferences = true,
};
var disabledView = retainerView with
{
    Characters = [retainerView.Characters[0] with { Retainers = [.. retainerView.Characters[0].Retainers, new("2", "Silver Wren", 1, true, 1, now, false, 2, 1, false, true)] }],
};
foreach (var width in new[] { 960, 650 })
{
    Render("account", width, Page.Account, view);
    Render("market", width, Page.Market, view);
    Render("lists", width, Page.Lists, view);
    Render("lists-route", width, Page.Lists, view);
    Render("lists-craft", width, Page.Lists, craftView);
    Render("lists-craft-subcrafts", width, Page.Lists, craftView);
    Render("lists-craft-by-job", width, Page.Lists, craftView);
    Render("lists-craft-route", width, Page.Lists, craftView);
    Render("history", width, Page.Notifications, view);
    Render("retainers", width, Page.Retainers, retainerView);
    Render("retainers-with-disabled", width, Page.Retainers, disabledView);
    Render("retainer-management", width, Page.Retainers, disabledView);
    Render("bell-missing", width, Page.Retainers, retainerView);
    Render("bell-menu", width, Page.Retainers, retainerView);
    Render("bell-refreshing", width, Page.Retainers, retainerView with { Registering = true });
}
Render("connect", 960, Page.Account, view with { Connected = false, Status = "Disconnected.", Session = null, Inbox = [] });
Render("approval", 650, Page.Account, view with { Connected = false, Status = "Waiting for browser approval.", Session = null, Inbox = [], HumanCode = "ABCD-EFGH", ApprovalUrl = "https://ultros.app/plugin/verify#pair=fixture" });
Render("empty-history", 650, Page.Notifications, view with { Inbox = [] });
Render("lists-banner", 960, Page.Lists, view);
Render("account-large-font", 650, Page.Account, view, 22);
Render("market-large-font", 650, Page.Market, view, 22);
Render("lists-large-font", 650, Page.Lists, view, 22);
Render("lists-route-large-font", 650, Page.Lists, view, 22);
Render("lists-craft-large-font", 650, Page.Lists, craftView, 22);
Render("retainers-large-font", 650, Page.Retainers, retainerView, 22);
Render("retainer-management-large-font", 650, Page.Retainers, disabledView, 22);
Console.WriteLine($"Native ImGui previews rendered to {Path.GetFullPath(output)}");

static string FixtureItemName(int id) => id switch
{
    8 => "Ice Crystal",
    5056 => "Courtly Lover's Shoes of Healing",
    9002 => "Courtly Lover's Earring of Healing",
    9101 => "Claro Walnut Thread",
    9102 => "Ra'Kaznar Ingot",
    9103 => "Black Star",
    9104 => "Silver Thread",
    9201 => "Claro Walnut Bark",
    9202 => "Ra'Kaznar Ore",
    9203 => "Cobalt Ore",
    10386 => "Grade 8 Dark Matter",
    _ => $"Item #{id}",
};
static string FixtureWorldName(int id) => id switch
{
    63 => "Gilgamesh",
    73 => "Adamantoise",
    _ => $"World #{id}",
};

unsafe void Render(string name, int width, Page selected, ClientView state, float fontSize = 17)
{
    ImGui.CreateContext();
    try
    {
        ImGui.StyleColorsDark();
        var io = ImGui.GetIO();
        io.DisplaySize = new(width, 720);
        io.DeltaTime = 1f / 60;
        io.IniFilename = null;
        if (name == "bell-menu") io.MousePos = new(35, 195);
        ushort* glyphs = stackalloc ushort[] { 0x20, 0xff, 0x2000, 0x206f, 0 };
        io.Fonts.AddFontFromFileTTF(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Fonts), "segoeui.ttf"), fontSize, default, glyphs);
        byte* atlas = null; int aw = 0, ah = 0;
        io.Fonts.GetTexDataAsRGBA32(0, &atlas, &aw, &ah);
        io.Fonts.SetTexID(0, new ImTextureID(1));
        var pixels = new byte[aw * ah * 4];
        Marshal.Copy((IntPtr)atlas, pixels, 0, pixels.Length);

        var recipes = new FixtureRecipeCatalog();
        var account = new AccountPanel();
        var marketPanel = new MarketPanel { ItemName = FixtureItemName, WorldName = FixtureWorldName, Marketable = _ => true };
        var listsPanel = new ListsPanel
        {
            ItemName = FixtureItemName,
            WorldName = FixtureWorldName,
            RecipeCatalog = recipes,
            SearchRecipes = recipes.SearchRecipes,
            CanSyncBagStock = () => true,
            SyncBagStock = _ => { },
        };
        if (name is "lists-route" or "lists-route-large-font")
            listsPanel.SetPreviewState(ListsPanel.ListsSubView.Route, false, state.SelectedList);
        else if (name is "lists-craft-route")
            listsPanel.SetPreviewState(ListsPanel.ListsSubView.Route, false, state.SelectedList);
        else if (name is "lists-craft-by-job")
            listsPanel.SetPreviewState(ListsPanel.ListsSubView.Craft, true, state.SelectedList, "2/step/9102");
        else if (name is "lists-craft" or "lists-craft-subcrafts" or "lists-craft-large-font")
            listsPanel.SetPreviewState(ListsPanel.ListsSubView.Craft, false, state.SelectedList, name == "lists-craft-subcrafts" ? "2/step/9102" : null);

        var history = new NotificationPanel();
        var retainers = new RetainerListingsPanel { ItemName = FixtureItemName, WorldName = FixtureWorldName, Marketable = _ => true };
        var bell = new BellRetainerPanel();
        var log = Path.GetFullPath(Path.Combine(output, $"{name}-{width}.txt"));
        File.WriteAllText(log, "");

        var unread = state.Inbox.Count(d => state.ReadNotificationIds?.Contains(d.Id) != true);
        // Let ImGui settle the child positions before capturing draw data.
        for (var frame = 0; frame < 3; frame++)
        {
            ImGui.NewFrame();
            ImGui.SetNextWindowPos(Vector2.Zero);
            ImGui.SetNextWindowSize(new(width, 720));
            ImGui.SetNextWindowBgAlpha(1);
            if (frame == 2) ImGui.LogToFile(0, log);
            UiKit.BeginStyle();
            ImGui.Begin("Ultros · native preview", ImGuiWindowFlags.NoResize | ImGuiWindowFlags.NoMove | ImGuiWindowFlags.NoCollapse);
            selected = UiKit.Navigation(selected, state.Connected, unread);
            UiKit.BeginContent();
            account.Track(state, selected == Page.Account);
            if (selected == Page.Account) account.Draw(state);
            else if (selected == Page.Market) marketPanel.Draw(state);
            else if (selected == Page.Lists)
            {
                if (name is "lists-craft-by-job" or "lists-craft-subcrafts" && frame > 0) ImGui.SetScrollY(340 * UiKit.Scale);
                listsPanel.Draw(state, name == "lists-banner" ? ("Inventory tracking paused after a game update", "FFXIV updated to build 2026.11.03. Confirm to resume.", "Resume inventory tracking for this build", () => { }) : null);
            }
            else if (name.StartsWith("bell-", StringComparison.Ordinal))
            {
                UiKit.Heading("Summoning-bell shortcut", "Component fixture; the in-game control is anchored to the bell window's header.");
                var observed = new ObservedCharacter("fixture-character", "Mira Example", 63, [new("fixture-retainer", "Copper Finch", 1, 0, true)]);
                if (name == "bell-missing") observed = observed with { Retainers = [.. observed.Retainers, new("new-fixture-retainer", "New Finch", 1, 1, true)] };
                if (name == "bell-menu" && frame > 0) ImGui.OpenPopup("Ultros retainers");
                bell.Draw(state, observed);
            }
            else if (name.StartsWith("retainer-management", StringComparison.Ordinal))
            {
                UiKit.Heading("Manage retainers", "Keep saved data when a retainer's contract ends.");
                foreach (var character in state.Characters) retainers.Manage(character, true);
            }
            else if (selected == Page.Retainers)
            {
                UiKit.Heading("Retainers", "Your uploaded listings, grouped by character and retainer.");
                retainers.Draw(state);
            }
            else history.Draw(state);
            UiKit.EndContent(state.Status);
            ImGui.End();
            UiKit.EndStyle();
            if (frame == 2) ImGui.LogFinish();
            ImGui.Render();
        }
        var drawn = File.ReadAllText(log);
        foreach (var secret in new[] { session.AccountName, session.AccountId, "ABCD-EFGH" })
            if (drawn.Contains(secret, StringComparison.Ordinal)) throw new Exception($"Private identity or pairing code visible in {name}.");
        if (state.Connected && selected == Page.Account && drawn.Contains("Connect to Ultros", StringComparison.Ordinal))
            throw new Exception("A connected account still shows setup instructions.");
        if (name == "market" && (!drawn.Contains("Gilgamesh", StringComparison.Ordinal) || !drawn.Contains("Adamantoise", StringComparison.Ordinal) || drawn.Contains("World ID", StringComparison.Ordinal)))
            throw new Exception("Market view did not resolve world names.");
        if (name == "lists" && (!drawn.Contains("Shopping Route (2)", StringComparison.Ordinal) || !drawn.Contains("377,260 gil", StringComparison.Ordinal)))
            throw new Exception("Lists view did not render estimate totals or Shopping Route count.");
        if (name == "lists-route" && (!drawn.Contains("Adamantoise", StringComparison.Ordinal) || !drawn.Contains("Gilgamesh", StringComparison.Ordinal)))
            throw new Exception("Shopping Route view did not group stops by world.");
        if (name == "lists-craft" && (!drawn.Contains("Finished targets", StringComparison.Ordinal) || !drawn.Contains("Sub-crafts", StringComparison.Ordinal)
            || !drawn.Contains("Used by:", StringComparison.Ordinal) || !drawn.Contains("Sync bag stock", StringComparison.Ordinal)
            || !drawn.Contains("READY", StringComparison.Ordinal) || !drawn.Contains("BUYING", StringComparison.Ordinal)))
            throw new Exception("Craft Copilot view did not render targets, sub-crafts, Used by attribution, or readiness badges.");
        if (name == "lists-craft-by-job" && (!drawn.Contains("WVR", StringComparison.Ordinal) || !drawn.Contains("ARM", StringComparison.Ordinal)))
            throw new Exception("Craft Copilot By job grouping did not render job sections.");
        if (name == "lists-craft-route" && (!drawn.Contains("Black Star", StringComparison.Ordinal) || !drawn.Contains("Ra'Kaznar Ore", StringComparison.Ordinal)))
            throw new Exception("Craft Copilot Shopping Route did not surface consolidated craft leaves and bought intermediates.");
        if (name == "history" && !drawn.Contains("Notifications (1)", StringComparison.Ordinal))
            throw new Exception("Unread notification count did not appear on the Notifications tab.");
        if (name == "retainers-with-disabled" && (!drawn.Contains("Show disabled retainers", StringComparison.Ordinal) || drawn.Contains("Silver Wren", StringComparison.Ordinal)))
            throw new Exception("Disabled retainers were not hidden with a recovery control.");
        if (name == "bell-missing" && !drawn.Contains("Refresh retainers (+1)", StringComparison.Ordinal))
            throw new Exception("Missing retainers did not expose one-click bell refresh.");
        WritePng(ImGui.GetDrawData(), pixels, aw, ah, width, 720, Path.Combine(output, $"{name}-{width}.png"));
    }
    finally { ImGui.DestroyContext(); }
}

static unsafe void WritePng(ImDrawDataPtr data, byte[] atlas, int aw, int ah, int width, int height, string path)
{
    var rgba = new byte[width * height * 4];
    for (var i = 0; i < rgba.Length; i += 4) { rgba[i] = 18; rgba[i + 1] = 18; rgba[i + 2] = 20; rgba[i + 3] = 255; }
    for (var n = 0; n < data.CmdListsCount; n++)
    {
        var list = new ImDrawListPtr(data.CmdLists[n]);
        foreach (var command in list.CmdBuffer)
        {
            if (command.UserCallback != null) continue;
            var clip = command.ClipRect;
            for (var index = 0u; index < command.ElemCount; index += 3)
            {
                var a = list.VtxBuffer[(int)(list.IdxBuffer[(int)(command.IdxOffset + index)] + command.VtxOffset)];
                var b = list.VtxBuffer[(int)(list.IdxBuffer[(int)(command.IdxOffset + index + 1)] + command.VtxOffset)];
                var c = list.VtxBuffer[(int)(list.IdxBuffer[(int)(command.IdxOffset + index + 2)] + command.VtxOffset)];
                var area = Cross(b.Pos - a.Pos, c.Pos - a.Pos); if (Math.Abs(area) < .001f) continue;
                var minX = (int)Math.Max(0, Math.Max(clip.X, Math.Floor(Math.Min(a.Pos.X, Math.Min(b.Pos.X, c.Pos.X)))));
                var maxX = (int)Math.Min(width, Math.Min(clip.Z, Math.Ceiling(Math.Max(a.Pos.X, Math.Max(b.Pos.X, c.Pos.X)))));
                var minY = (int)Math.Max(0, Math.Max(clip.Y, Math.Floor(Math.Min(a.Pos.Y, Math.Min(b.Pos.Y, c.Pos.Y)))));
                var maxY = (int)Math.Min(height, Math.Min(clip.W, Math.Ceiling(Math.Max(a.Pos.Y, Math.Max(b.Pos.Y, c.Pos.Y)))));
                for (var y = minY; y < maxY; y++)
                    for (var x = minX; x < maxX; x++)
                    {
                        var p = new Vector2(x + .5f, y + .5f);
                        var u = Cross(b.Pos - p, c.Pos - p) / area; var v = Cross(c.Pos - p, a.Pos - p) / area; var w = 1 - u - v;
                        if (u < 0 || v < 0 || w < 0) continue;
                        var uv = u * a.Uv + v * b.Uv + w * c.Uv;
                        var tx = Math.Clamp((int)(uv.X * aw), 0, aw - 1); var ty = Math.Clamp((int)(uv.Y * ah), 0, ah - 1);
                        var tex = (ty * aw + tx) * 4; var dst = (y * width + x) * 4;
                        float Color(int channel) => u * ((a.Col >> (channel * 8)) & 255) + v * ((b.Col >> (channel * 8)) & 255) + w * ((c.Col >> (channel * 8)) & 255);
                        var alpha = Color(3) / 255 * atlas[tex + 3] / 255;
                        for (var channel = 0; channel < 3; channel++) rgba[dst + channel] = (byte)(Color(channel) * atlas[tex + channel] / 255 * alpha + rgba[dst + channel] * (1 - alpha));
                    }
            }
        }
    }
    using var bitmap = new Bitmap(width, height, PixelFormat.Format32bppArgb);
    var locked = bitmap.LockBits(new Rectangle(0, 0, width, height), ImageLockMode.WriteOnly, PixelFormat.Format32bppArgb);
    for (var i = 0; i < rgba.Length; i += 4) (rgba[i], rgba[i + 2]) = (rgba[i + 2], rgba[i]);
    try { Marshal.Copy(rgba, 0, locked.Scan0, rgba.Length); } finally { bitmap.UnlockBits(locked); }
    bitmap.Save(path, ImageFormat.Png);
}
static float Cross(Vector2 a, Vector2 b) => a.X * b.Y - a.Y * b.X;

file sealed class FixtureRecipeCatalog : ICraftRecipeSource
{
    private readonly Dictionary<int, RecipeInfo> recipes = new()
    {
        [1001] = new(1001, 5056, 1, "WVR", 100, [new(9101, 2), new(9102, 2), new(8, 2)]),
        [1002] = new(1002, 9002, 1, "GSM", 100, [new(9102, 1), new(9103, 1), new(8, 2)]),
        [2001] = new(2001, 9101, 1, "WVR", 98, [new(9104, 1), new(9201, 2)]),
        [2002] = new(2002, 9102, 1, "ARM", 98, [new(9202, 2)]),
        [2003] = new(2003, 9103, 1, "GSM", 96, [new(9202, 2)]),
        [2004] = new(2004, 9104, 1, "WVR", 94, [new(9203, 2)]),
    };

    public RecipeInfo? GetRecipe(int recipeId) => recipes.GetValueOrDefault(recipeId);
    public RecipeInfo? GetDefaultRecipeForItem(int itemId) =>
        recipes.Values.Where(r => r.OutputItemId == itemId).OrderBy(r => r.RecipeId).FirstOrDefault();
    public bool IsCrystal(int itemId) => itemId == 8;

    public (int RecipeId, int ItemId, string Name, string JobAbbr, int Level)[] SearchRecipes(string term) =>
        recipes.Values
            .Select(r => (r.RecipeId, r.OutputItemId, Name: r.OutputItemId switch
            {
                5056 => "Courtly Lover's Shoes of Healing",
                9002 => "Courtly Lover's Earring of Healing",
                9101 => "Claro Walnut Thread",
                9102 => "Ra'Kaznar Ingot",
                9103 => "Black Star",
                _ => "Silver Thread",
            }, r.JobAbbr, r.RecipeLevel))
            .Where(r => r.Name.Contains(term, StringComparison.OrdinalIgnoreCase))
            .ToArray();
}
