using Ultros.Lists;
namespace Ultros.Client;

/// <summary>The displayed document instance, independent of its changing causal version.</summary>
public sealed record CartCommandTarget(string AccountId, string DeviceId, int ListId, Guid InstanceId)
{
    public static CartCommandTarget? Capture(ClientView view) =>
        view.Connected && view.Session is { } session && view.SelectedList is > 0 && view.Cart is not null
            && view.CartInstance is { } instance && instance != Guid.Empty
            ? new(session.AccountId, session.Id, view.SelectedList.Value, instance) : null;
    public bool Matches(ClientView view) => this == Capture(view);
    internal bool Matches(DeviceSession? session, ConnectedCart? cart) => session is not null && cart is not null
        && session.AccountId == AccountId && session.Id == DeviceId && cart.Id == ListId
        && cart.InstanceId == InstanceId && !cart.AccessDenied;
    public override string ToString() => "[Cart command destination]";
}

/// <summary>Recovery choices also bind the exact local state and server candidate shown.</summary>
public sealed record RecoveryChoiceTarget(CartCommandTarget Cart, string Version, long Revision)
{
    public static RecoveryChoiceTarget? Capture(ClientView view) =>
        view.Cart?.RecoveryReview == true && CartCommandTarget.Capture(view) is { } cart
            ? new(cart, view.Cart.Version, view.RecoveryRevision) : null;
    internal bool Matches(DeviceSession? session, ConnectedCart? cart) => Cart.Matches(session, cart)
        && cart!.Document.View.RecoveryReview && cart.Document.View.Version == Version
        && cart.RecoveryRevision == Revision;
    public override string ToString() => "[Cart recovery choice]";
}

/// <summary>Execute only after checking the queued command against the current owner and document.</summary>
internal static class CartCommands
{
    internal static bool Edit(CartCommandTarget? target, DeviceSession? session, ConnectedCart? cart, bool canEdit, object edit)
    {
        if (!canEdit || target?.Matches(session, cart) != true) return false;
        cart!.Edit(edit);
        return true;
    }
    internal static bool Recover(RecoveryChoiceTarget? target, DeviceSession? session, ConnectedCart? cart, bool useServer)
    {
        if (target?.Matches(session, cart) != true || useServer && !cart!.Document.View.HasServerCopy) return false;
        cart!.ResolveRecovery(useServer);
        return true;
    }
}
