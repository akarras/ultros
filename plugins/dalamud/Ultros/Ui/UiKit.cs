using System.Numerics;
using Dalamud.Bindings.ImGui;

namespace UltrosPlugin.Ui;

internal enum Page { Market, Lists, Retainers, Notifications, Account, Settings }

internal readonly record struct KpiCard(string Label, string Value, string? Subtext = null, Vector4? Accent = null);

internal static class UiKit
{
    internal static float Scale => ImGui.GetFontSize() / 16f;
    internal static readonly Vector4 Accent = new(0.68f, 0.56f, 0.96f, 1f);
    internal static readonly Vector4 Success = new(0.36f, 0.84f, 0.55f, 1f);
    internal static readonly Vector4 Warning = new(0.95f, 0.76f, 0.32f, 1f);
    internal static readonly Vector4 Danger = new(0.93f, 0.42f, 0.42f, 1f);
    internal static readonly Vector4 HqGold = new(0.96f, 0.80f, 0.36f, 1f);
    internal static readonly Vector4 CardBg = new(0.13f, 0.13f, 0.17f, 0.92f);
    internal static readonly Vector4 BannerWarnBg = new(0.26f, 0.21f, 0.10f, 0.92f);
    internal static readonly Vector4 BannerInfoBg = new(0.14f, 0.15f, 0.24f, 0.92f);

    internal static void BeginStyle()
    {
        ImGui.PushStyleVar(ImGuiStyleVar.WindowPadding, new Vector2(16, 14) * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.FramePadding, new Vector2(10, 6) * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.ItemSpacing, new Vector2(10, 9) * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.FrameRounding, 4 * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.ChildRounding, 6 * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.CellPadding, new Vector2(10, 7) * Scale);
    }

    internal static void EndStyle() => ImGui.PopStyleVar(6);

    internal static void Muted(string text) =>
        ImGui.TextColored(ImGui.GetStyle().Colors[(int)ImGuiCol.TextDisabled], text);

    internal static void Hint(string text)
    {
        ImGui.PushStyleColor(ImGuiCol.Text, ImGui.GetStyle().Colors[(int)ImGuiCol.TextDisabled]);
        ImGui.TextWrapped(text);
        ImGui.PopStyleColor();
    }

    internal static void Heading(string title, string? hint = null)
    {
        ImGui.SetWindowFontScale(1.45f);
        ImGui.TextUnformatted(title);
        ImGui.SetWindowFontScale(1);
        if (hint is not null) Hint(hint);
        ImGui.Spacing();
        ImGui.Separator();
        ImGui.Spacing();
    }

    internal static void Section(string title)
    {
        ImGui.Spacing();
        ImGui.Separator();
        ImGui.Spacing();
        ImGui.SetWindowFontScale(1.25f);
        ImGui.TextUnformatted(title);
        ImGui.SetWindowFontScale(1);
        ImGui.Spacing();
    }

    internal static void Inline(float room = 180)
    {
        var right = ImGui.GetWindowPos().X + ImGui.GetWindowContentRegionMax().X;
        if (ImGui.GetItemRectMax().X + room * Scale < right) ImGui.SameLine();
    }

    internal static void Empty(string title, string help)
    {
        ImGui.Spacing();
        ImGui.Spacing();
        ImGui.TextUnformatted(title);
        Hint(help);
    }

    internal static void Pill(string text, Vector4 color)
    {
        ImGui.TextColored(color, $"[{text}]");
    }

    internal static void QualityBadge(bool hq)
    {
        if (hq) ImGui.TextColored(HqGold, "HQ");
        else Muted("NQ");
    }

    internal static void QualityBadge(string quality)
    {
        if (quality.Equals("hq", StringComparison.OrdinalIgnoreCase)) ImGui.TextColored(HqGold, "HQ");
        else if (quality.Equals("nq", StringComparison.OrdinalIgnoreCase)) Muted("NQ");
        else Muted("Any");
    }

    internal static void ItemIcon(Func<int, bool, ImTextureID?>? iconLookup, int itemId, bool hq, float logicalSize = 22f)
    {
        if (iconLookup is null) return;
        var size = logicalSize * Scale;
        if (iconLookup(itemId, hq) is { } texture && texture != default)
        {
            ImGui.Image(texture, new Vector2(size, size));
            ImGui.SameLine(0, 6 * Scale);
        }
    }

    internal static void ProgressBar(float fraction, string overlay)
    {
        ImGui.PushStyleColor(ImGuiCol.PlotHistogram, Accent);
        try { ImGui.ProgressBar(Math.Clamp(fraction, 0f, 1f), new Vector2(-1, 22 * Scale), overlay); }
        finally { ImGui.PopStyleColor(); }
    }

    internal static void KpiRow(params KpiCard[] cards)
    {
        if (cards.Length == 0) return;
        var avail = ImGui.GetContentRegionAvail().X;
        var minCardWidth = 148 * Scale;
        var columns = Math.Max(1, Math.Min(cards.Length, (int)Math.Floor((avail + 8 * Scale) / (minCardWidth + 8 * Scale))));
        if (!ImGui.BeginTable("##kpi-row", columns, ImGuiTableFlags.SizingStretchSame)) return;
        try
        {
            for (var i = 0; i < cards.Length; i++)
            {
                if (i % columns == 0) ImGui.TableNextRow();
                ImGui.TableNextColumn();
                var card = cards[i];
                ImGui.PushStyleColor(ImGuiCol.ChildBg, CardBg);
                ImGui.PushStyleVar(ImGuiStyleVar.WindowPadding, new Vector2(12, 8) * Scale);
                ImGui.PushStyleVar(ImGuiStyleVar.ItemSpacing, new Vector2(6, 3) * Scale);
                try
                {
                    var cardHeight = (card.Subtext is not null ? 74 : 56) * Scale;
                    ImGui.BeginChild($"##kpi-{i}", new Vector2(-1, cardHeight), true, ImGuiWindowFlags.NoScrollbar | ImGuiWindowFlags.NoScrollWithMouse);
                    try
                    {
                        Muted(card.Label);
                        ImGui.SetWindowFontScale(1.18f);
                        if (card.Accent is { } accent) ImGui.TextColored(accent, card.Value);
                        else ImGui.TextUnformatted(card.Value);
                        ImGui.SetWindowFontScale(1f);
                        if (card.Subtext is not null) Muted(card.Subtext);
                    }
                    finally { ImGui.EndChild(); }
                }
                finally
                {
                    ImGui.PopStyleVar(2);
                    ImGui.PopStyleColor();
                }
            }
        }
        finally { ImGui.EndTable(); }
    }

    internal static void Banner(string id, string title, string message, string? actionLabel = null, Action? onAction = null, bool warning = true)
    {
        ImGui.PushStyleColor(ImGuiCol.ChildBg, warning ? BannerWarnBg : BannerInfoBg);
        ImGui.PushStyleVar(ImGuiStyleVar.WindowPadding, new Vector2(12, 9) * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.ItemSpacing, new Vector2(8, 5) * Scale);
        try
        {
            var height = (actionLabel is not null ? 96 : 64) * Scale;
            ImGui.BeginChild($"##banner-{id}", new Vector2(-1, height), true, ImGuiWindowFlags.NoScrollbar);
            try
            {
                ImGui.TextColored(warning ? Warning : Accent, title);
                ImGui.TextWrapped(message);
                if (actionLabel is not null && onAction is not null)
                {
                    if (ImGui.Button(actionLabel)) onAction();
                }
            }
            finally { ImGui.EndChild(); }
        }
        finally
        {
            ImGui.PopStyleVar(2);
            ImGui.PopStyleColor();
        }
        ImGui.Spacing();
    }

    internal static void BeginContent()
    {
        ImGui.BeginChild("page", Vector2.Zero, false);
        ImGui.BeginChild("content", new Vector2(0, -3 * ImGui.GetTextLineHeightWithSpacing()), false);
    }

    internal static void EndContent(string status)
    {
        ImGui.EndChild();
        ImGui.Separator();
        Hint(status);
        ImGui.EndChild();
    }

    internal static Page Navigation(Page current, bool connected, int unreadNotifications, bool activate = false)
    {
        if (ImGui.BeginTabBar("ultros-pages", ImGuiTabBarFlags.FittingPolicyScroll))
        {
            try
            {
                var requested = current;
                var accountLabel = connected ? "Account###tab-account" : "Account (Offline)###tab-account";
                var notifLabel = unreadNotifications > 0 ? $"Notifications ({unreadNotifications})###tab-notifications" : "Notifications###tab-notifications";
                foreach (var (page, label) in new[]
                {
                    (Page.Account, accountLabel),
                    (Page.Market, "Market###tab-market"),
                    (Page.Lists, "Lists###tab-lists"),
                    (Page.Retainers, "Retainers###tab-retainers"),
                    (Page.Notifications, notifLabel),
                    (Page.Settings, "Settings###tab-settings"),
                })
                {
                    if (ImGui.BeginTabItem(label, (activate || ImGui.IsWindowAppearing()) && requested == page ? ImGuiTabItemFlags.SetSelected : ImGuiTabItemFlags.None))
                    {
                        current = page;
                        ImGui.EndTabItem();
                    }
                }
            }
            finally { ImGui.EndTabBar(); }
        }
        return current;
    }
}
