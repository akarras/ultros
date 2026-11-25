using System.Numerics;
using Dalamud.Bindings.ImGui;

namespace UltrosPlugin.Ui;

internal enum Page { Market, Lists, Retainers, Notifications, Account, Settings }

internal readonly record struct KpiCard(string Label, string Value, string? Subtext = null, Vector4? Accent = null, float? Progress = null);

internal static class UiKit
{
    internal static float Scale => ImGui.GetFontSize() / 16f;

    // Ultros dark-slate & purple palette
    internal static readonly Vector4 Accent = new(0.69f, 0.56f, 0.98f, 1f);
    internal static readonly Vector4 AccentBtn = new(0.45f, 0.34f, 0.78f, 1f);
    internal static readonly Vector4 AccentBtnHover = new(0.54f, 0.42f, 0.88f, 1f);
    internal static readonly Vector4 AccentBtnActive = new(0.39f, 0.29f, 0.69f, 1f);

    internal static readonly Vector4 Surface = new(0.11f, 0.11f, 0.15f, 1f);
    internal static readonly Vector4 CardBg = new(0.14f, 0.14f, 0.19f, 0.96f);
    internal static readonly Vector4 CardUnreadBorder = new(0.56f, 0.44f, 0.88f, 0.85f);
    internal static readonly Vector4 InputBg = new(0.16f, 0.17f, 0.23f, 1f);
    internal static readonly Vector4 InputHover = new(0.21f, 0.22f, 0.30f, 1f);
    internal static readonly Vector4 InputActive = new(0.25f, 0.26f, 0.36f, 1f);
    internal static readonly Vector4 ControlBg = new(0.20f, 0.21f, 0.29f, 1f);
    internal static readonly Vector4 ControlHover = new(0.27f, 0.28f, 0.39f, 1f);
    internal static readonly Vector4 ControlActive = new(0.33f, 0.34f, 0.47f, 1f);
    internal static readonly Vector4 Border = new(0.25f, 0.25f, 0.35f, 0.85f);
    internal static readonly Vector4 HeaderBg = new(0.16f, 0.17f, 0.24f, 1f);
    internal static readonly Vector4 HeaderHover = new(0.23f, 0.24f, 0.34f, 1f);
    internal static readonly Vector4 HeaderActive = new(0.29f, 0.30f, 0.42f, 1f);

    internal static readonly Vector4 Success = new(0.36f, 0.84f, 0.56f, 1f);
    internal static readonly Vector4 Warning = new(0.95f, 0.76f, 0.32f, 1f);
    internal static readonly Vector4 Danger = new(0.93f, 0.42f, 0.44f, 1f);
    internal static readonly Vector4 DangerHover = new(0.62f, 0.21f, 0.25f, 1f);
    internal static readonly Vector4 DangerActive = new(0.74f, 0.25f, 0.29f, 1f);
    internal static readonly Vector4 HqGold = new(0.96f, 0.80f, 0.36f, 1f);
    internal static readonly Vector4 BannerWarnBg = new(0.25f, 0.20f, 0.10f, 0.94f);
    internal static readonly Vector4 BannerInfoBg = new(0.15f, 0.14f, 0.24f, 0.94f);

    private const int StyleVarCount = 7;
    private const int StyleColorCount = 29;

    internal static void BeginStyle()
    {
        ImGui.PushStyleVar(ImGuiStyleVar.WindowPadding, new Vector2(14, 12) * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.FramePadding, new Vector2(9, 5) * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.ItemSpacing, new Vector2(8, 7) * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.FrameRounding, 4 * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.ChildRounding, 6 * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.TabRounding, 4 * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.CellPadding, new Vector2(8, 6) * Scale);

        ImGui.PushStyleColor(ImGuiCol.WindowBg, Surface);
        ImGui.PushStyleColor(ImGuiCol.TitleBg, new Vector4(0.13f, 0.13f, 0.19f, 1f));
        ImGui.PushStyleColor(ImGuiCol.TitleBgActive, new Vector4(0.19f, 0.16f, 0.30f, 1f));
        ImGui.PushStyleColor(ImGuiCol.TitleBgCollapsed, new Vector4(0.11f, 0.11f, 0.16f, 0.92f));
        ImGui.PushStyleColor(ImGuiCol.PopupBg, new Vector4(0.13f, 0.13f, 0.18f, 0.98f));
        ImGui.PushStyleColor(ImGuiCol.Border, Border);
        ImGui.PushStyleColor(ImGuiCol.FrameBg, InputBg);
        ImGui.PushStyleColor(ImGuiCol.FrameBgHovered, InputHover);
        ImGui.PushStyleColor(ImGuiCol.FrameBgActive, InputActive);
        ImGui.PushStyleColor(ImGuiCol.Button, ControlBg);
        ImGui.PushStyleColor(ImGuiCol.ButtonHovered, ControlHover);
        ImGui.PushStyleColor(ImGuiCol.ButtonActive, ControlActive);
        ImGui.PushStyleColor(ImGuiCol.Header, HeaderBg);
        ImGui.PushStyleColor(ImGuiCol.HeaderHovered, HeaderHover);
        ImGui.PushStyleColor(ImGuiCol.HeaderActive, HeaderActive);
        ImGui.PushStyleColor(ImGuiCol.Tab, new Vector4(0.15f, 0.16f, 0.22f, 1f));
        ImGui.PushStyleColor(ImGuiCol.TabHovered, AccentBtnHover);
        ImGui.PushStyleColor(ImGuiCol.TabActive, AccentBtn);
        ImGui.PushStyleColor(ImGuiCol.TabUnfocused, new Vector4(0.13f, 0.14f, 0.19f, 1f));
        ImGui.PushStyleColor(ImGuiCol.TabUnfocusedActive, new Vector4(0.32f, 0.25f, 0.54f, 1f));
        ImGui.PushStyleColor(ImGuiCol.CheckMark, Accent);
        ImGui.PushStyleColor(ImGuiCol.SliderGrab, AccentBtn);
        ImGui.PushStyleColor(ImGuiCol.SliderGrabActive, Accent);
        ImGui.PushStyleColor(ImGuiCol.Separator, Border);
        ImGui.PushStyleColor(ImGuiCol.TableHeaderBg, new Vector4(0.16f, 0.17f, 0.24f, 1f));
        ImGui.PushStyleColor(ImGuiCol.ScrollbarBg, new Vector4(0.09f, 0.09f, 0.13f, 0.9f));
        ImGui.PushStyleColor(ImGuiCol.ScrollbarGrab, new Vector4(0.24f, 0.24f, 0.34f, 1f));
        ImGui.PushStyleColor(ImGuiCol.ScrollbarGrabHovered, new Vector4(0.34f, 0.32f, 0.48f, 1f));
        ImGui.PushStyleColor(ImGuiCol.ScrollbarGrabActive, AccentBtn);
    }

    internal static void EndStyle()
    {
        ImGui.PopStyleColor(StyleColorCount);
        ImGui.PopStyleVar(StyleVarCount);
    }

    internal static bool PrimaryButton(string label, bool small = false)
    {
        ImGui.PushStyleColor(ImGuiCol.Button, AccentBtn);
        ImGui.PushStyleColor(ImGuiCol.ButtonHovered, AccentBtnHover);
        ImGui.PushStyleColor(ImGuiCol.ButtonActive, AccentBtnActive);
        try { return small ? ImGui.SmallButton(label) : ImGui.Button(label); }
        finally { ImGui.PopStyleColor(3); }
    }

    internal static bool ToggleChip(string label, bool active)
    {
        if (active)
        {
            ImGui.PushStyleColor(ImGuiCol.Button, AccentBtn);
            ImGui.PushStyleColor(ImGuiCol.ButtonHovered, AccentBtnHover);
            ImGui.PushStyleColor(ImGuiCol.ButtonActive, AccentBtnActive);
        }
        try { return ImGui.Button(label); }
        finally { if (active) ImGui.PopStyleColor(3); }
    }

    internal static bool DangerButton(string label, bool small = false)
    {
        ImGui.PushStyleColor(ImGuiCol.Button, new Vector4(0.24f, 0.16f, 0.19f, 1f));
        ImGui.PushStyleColor(ImGuiCol.ButtonHovered, DangerHover);
        ImGui.PushStyleColor(ImGuiCol.ButtonActive, DangerActive);
        try { return small ? ImGui.SmallButton(label) : ImGui.Button(label); }
        finally { ImGui.PopStyleColor(3); }
    }

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
        ImGui.SetWindowFontScale(1.32f);
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
        ImGui.SetWindowFontScale(1.15f);
        ImGui.TextUnformatted(title);
        ImGui.SetWindowFontScale(1);
    }

    internal static bool Inline(float room = 180)
    {
        var right = ImGui.GetWindowPos().X + ImGui.GetWindowContentRegionMax().X;
        if (ImGui.GetItemRectMax().X + room * Scale < right)
        {
            ImGui.SameLine();
            return true;
        }
        return false;
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

    internal static void ItemIcon(Func<int, bool, ImTextureID?>? iconLookup, int itemId, bool hq, float logicalSize = 20f)
    {
        if (iconLookup is null) return;
        var size = logicalSize * Scale;
        if (iconLookup(itemId, hq) is { } texture && texture != default)
        {
            ImGui.Image(texture, new Vector2(size, size));
            ImGui.SameLine(0, 6 * Scale);
        }
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
                var rowStart = (i / columns) * columns;
                var rowEnd = Math.Min(cards.Length, rowStart + columns);
                var rowBaseHeight = 52;
                for (var j = rowStart; j < rowEnd; j++)
                {
                    if (cards[j].Progress is not null) rowBaseHeight = Math.Max(rowBaseHeight, 78);
                    else if (cards[j].Subtext is not null) rowBaseHeight = Math.Max(rowBaseHeight, 68);
                }
                ImGui.PushStyleColor(ImGuiCol.ChildBg, CardBg);
                ImGui.PushStyleVar(ImGuiStyleVar.WindowPadding, new Vector2(11, 7) * Scale);
                ImGui.PushStyleVar(ImGuiStyleVar.ItemSpacing, new Vector2(6, 3) * Scale);
                try
                {
                    var cardHeight = rowBaseHeight * Scale;
                    ImGui.BeginChild($"##kpi-{i}", new Vector2(-1, cardHeight), true, ImGuiWindowFlags.NoScrollbar | ImGuiWindowFlags.NoScrollWithMouse);
                    try
                    {
                        Muted(card.Label);
                        ImGui.SetWindowFontScale(1.16f);
                        if (card.Accent is { } accent) ImGui.TextColored(accent, card.Value);
                        else ImGui.TextUnformatted(card.Value);
                        ImGui.SetWindowFontScale(1f);
                        if (card.Progress is { } pct)
                        {
                            ImGui.PushStyleColor(ImGuiCol.PlotHistogram, Accent);
                            try { ImGui.ProgressBar(Math.Clamp(pct, 0f, 1f), new Vector2(-1, 6 * Scale), ""); }
                            finally { ImGui.PopStyleColor(); }
                        }
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
        ImGui.PushStyleVar(ImGuiStyleVar.WindowPadding, new Vector2(12, 8) * Scale);
        ImGui.PushStyleVar(ImGuiStyleVar.ItemSpacing, new Vector2(8, 4) * Scale);
        try
        {
            var height = (actionLabel is not null ? 92 : 60) * Scale;
            ImGui.BeginChild($"##banner-{id}", new Vector2(-1, height), true, ImGuiWindowFlags.NoScrollbar);
            try
            {
                ImGui.TextColored(warning ? Warning : Accent, title);
                ImGui.TextWrapped(message);
                if (actionLabel is not null && onAction is not null)
                {
                    if (PrimaryButton(actionLabel)) onAction();
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
        ImGui.BeginChild("content", new Vector2(0, -2.2f * ImGui.GetTextLineHeightWithSpacing()), false);
    }

    internal static void EndContent(string status, string? notice = null, Action? onDismissNotice = null)
    {
        ImGui.EndChild();
        ImGui.Separator();
        if (!string.IsNullOrEmpty(notice))
        {
            if (onDismissNotice is not null)
            {
                if (ImGui.SmallButton("Dismiss")) onDismissNotice();
                ImGui.SameLine();
            }
            ImGui.TextColored(Warning, notice);
        }
        else
        {
            Muted(status);
        }
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
                    (Page.Market, "Market###tab-market"),
                    (Page.Lists, "Lists###tab-lists"),
                    (Page.Retainers, "Retainers###tab-retainers"),
                    (Page.Notifications, notifLabel),
                    (Page.Account, accountLabel),
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
