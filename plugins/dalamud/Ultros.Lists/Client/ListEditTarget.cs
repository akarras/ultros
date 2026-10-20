namespace Ultros.Client;

/// <summary>Bind a queued edit to the destination the player actually saw.</summary>
public sealed record ListEditTarget(string AccountId, string DeviceId, int ListId, Guid? InstanceId = null)
{
    public static ListEditTarget? Capture(ClientView view) => Capture(view.Session, view.SelectedList, view.CanEdit, view.CartInstance);
    public static ListEditTarget? Capture(DeviceSession? session, int? list, bool canEdit, Guid? instance = null) =>
        canEdit && list is > 0 && session is not null && session.Scopes.Contains("lists:write", StringComparer.Ordinal)
            ? new(session.AccountId, session.Id, list.Value, instance) : null;
    public static ListEditTarget? CaptureDestination(ClientView view) =>
        view.Connected && view.Session is { } session && view.SelectedList is > 0
            ? new(session.AccountId, session.Id, view.SelectedList.Value, view.CartInstance) : null;
    // A failed checkpoint pauses edits, but does not change the inventory baseline.
    public static ListEditTarget? CaptureInventory(ClientView view) =>
        view.Cart is { Ready: true, RecoveryReview: false }
            && view.Lists.Any(list => list.List.Id == view.SelectedList && list.CanWrite)
            ? Capture(view.Session, view.SelectedList, view.Connected, view.CartInstance) : null;
    public bool MatchesDestination(DeviceSession? session, int? list, Guid? instance = null) =>
        session is not null && AccountId == session.AccountId && DeviceId == session.Id && ListId == list && InstanceId == instance;
    public bool Matches(ClientView view) => Matches(view.Session, view.SelectedList, view.CanEdit, view.CartInstance);
    public bool Matches(DeviceSession? session, int? list, bool canEdit, Guid? instance = null) => this == Capture(session, list, canEdit, instance);
    public override string ToString() => "[List edit destination]";
}
