using System.Numerics;
using Dalamud.Bindings.ImGui;
using Ultros.Client;

namespace UltrosPlugin.Ui;

internal sealed class NotificationPanel
{
    internal Action Test { get; init; } = () => { };
    internal Action Refresh { get; init; } = () => { };
    internal Action<string> MarkRead { get; init; } = _ => { };
    internal Action MarkAllRead { get; init; } = () => { };
    internal Action<DeviceSession?, string[]> Delete { get; init; } = (_, _) => { };
    internal Action<int> OpenItem { get; init; } = _ => { };
    internal Action<int> OpenList { get; init; } = _ => { };
    private DeviceSession? clearingSession;
    private string[] clearing = [];

    internal void Draw(ClientView v)
    {
        var unread = v.Inbox.Count(d => v.ReadNotificationIds?.Contains(d.Id) != true);

        ImGui.BeginDisabled(!v.Connected || unread == 0);
        if (ImGui.Button("Mark all read")) MarkAllRead();
        ImGui.EndDisabled();

        ImGui.SameLine();
        ImGui.BeginDisabled(!v.Connected);
        if (ImGui.Button("Refresh##notif")) Refresh();
        ImGui.EndDisabled();

        if (v.Inbox.Length > 0)
        {
            ImGui.SameLine();
            if (UiKit.DangerButton("Clear history..."))
            {
                clearing = v.Inbox.Select(d => d.Id).ToArray();
                clearingSession = v.Session;
                ImGui.OpenPopup("Clear notification history?");
            }
        }

        UiKit.Inline(220);
        UiKit.Muted($"{v.Inbox.Length} saved · {unread} unread · {v.NotificationTransport}");

        if (ImGui.BeginPopupModal("Clear notification history?", ImGuiWindowFlags.AlwaysAutoResize))
        {
            ImGui.TextUnformatted($"Remove {clearing.Length} notifications from this client's history?");
            ImGui.TextUnformatted("New alerts and your alert rules are kept.");
            if (v.Session?.Id != clearingSession?.Id) { clearing = []; ImGui.CloseCurrentPopup(); }
            else if (UiKit.DangerButton("Clear history")) { Delete(clearingSession, clearing); ImGui.CloseCurrentPopup(); }
            ImGui.SameLine();
            if (ImGui.Button("Cancel")) ImGui.CloseCurrentPopup();
            ImGui.EndPopup();
        }

        if (v.Inbox.Length == 0)
        {
            UiKit.Empty("You're all caught up", v.Connected
                ? "New alerts will appear here and in game chat (when enabled in Settings)."
                : "Connect your account to receive alerts here.");
            if (v.Connected)
            {
                ImGui.Spacing();
                if (ImGui.Button("Send test alert")) Test();
            }
            return;
        }

        ImGui.Spacing();
        foreach (var entry in v.Inbox.Reverse())
        {
            ImGui.PushID(entry.Id);
            try
            {
                var isRead = v.ReadNotificationIds?.Contains(entry.Id) == true;
                ImGui.PushStyleColor(ImGuiCol.ChildBg, UiKit.CardBg);
                ImGui.PushStyleColor(ImGuiCol.Border, isRead ? UiKit.Border : UiKit.CardUnreadBorder);
                ImGui.PushStyleVar(ImGuiStyleVar.WindowPadding, new Vector2(12, 8) * UiKit.Scale);
                ImGui.PushStyleVar(ImGuiStyleVar.ItemSpacing, new Vector2(8, 5) * UiKit.Scale);
                try
                {
                    var cardHeight = EstimateCardHeight(entry.Body);
                    ImGui.BeginChild("##notif-card", new Vector2(-1, cardHeight), true, ImGuiWindowFlags.NoScrollbar | ImGuiWindowFlags.NoScrollWithMouse);
                    try
                    {
                        if (!isRead)
                        {
                            UiKit.Pill(entry.Kind == "test" ? "TEST" : "NEW", UiKit.Accent);
                            ImGui.SameLine();
                        }
                        ImGui.TextUnformatted(entry.Title);
                        if (v.NotificationReceivedAt?.TryGetValue(entry.Id, out var receivedAt) == true)
                        {
                            UiKit.Inline(110);
                            UiKit.Muted(TimestampFormat.FormatRelativeOrLocal(receivedAt));
                        }

                        if (isRead) UiKit.Hint(entry.Body);
                        else ImGui.TextWrapped(entry.Body);

                        var hasAction = false;
                        if (entry.Action.Kind == "open_item" && entry.Action.ItemId is { } item)
                        {
                            if (UiKit.PrimaryButton("View item", small: true)) OpenItem(item);
                            hasAction = true;
                        }
                        else if (entry.Action.Kind == "open_list" && entry.Action.ListId is { } list)
                        {
                            if (UiKit.PrimaryButton("Open list", small: true)) OpenList(list);
                            hasAction = true;
                        }
                        if (!isRead && v.Connected && v.Scopes.Contains("notifications:mark-read"))
                        {
                            if (hasAction) ImGui.SameLine();
                            if (ImGui.SmallButton("Mark read")) MarkRead(entry.Id);
                            hasAction = true;
                        }
                        if (hasAction) ImGui.SameLine();
                        if (UiKit.DangerButton("Delete", small: true)) Delete(v.Session, [entry.Id]);
                    }
                    finally { ImGui.EndChild(); }
                }
                finally
                {
                    ImGui.PopStyleVar(2);
                    ImGui.PopStyleColor(2);
                }
            }
            finally { ImGui.PopID(); }
        }
    }

    private static float EstimateCardHeight(string body)
    {
        var avail = Math.Max(260 * UiKit.Scale, ImGui.GetContentRegionAvail().X - 24 * UiKit.Scale);
        var wrapped = ImGui.CalcTextSize(body, false, avail).Y;
        var line = ImGui.GetTextLineHeightWithSpacing();
        return wrapped + line * 2.25f + 16 * UiKit.Scale;
    }
}
