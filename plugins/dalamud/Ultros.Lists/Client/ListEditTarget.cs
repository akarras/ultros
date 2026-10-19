namespace Ultros.Client;

/// <summary>Bind a queued edit to the destination the player actually saw.</summary>
public sealed record ListEditTarget(string AccountId, string DeviceId, int ListId)
{
    public static ListEditTarget? Capture(ClientView view) => Capture(view.Session, view.SelectedList, view.CanEdit);
    public static ListEditTarget? Capture(DeviceSession? session, int? list, bool canEdit) =>
        canEdit && list is > 0 && session is not null && session.Scopes.Contains("lists:write", StringComparer.Ordinal)
            ? new(session.AccountId, session.Id, list.Value) : null;
    public bool Matches(ClientView view) => Matches(view.Session, view.SelectedList, view.CanEdit);
    public bool Matches(DeviceSession? session, int? list, bool canEdit) => this == Capture(session, list, canEdit);
    public override string ToString() => "[List edit destination]";
}
