using System.Text.Json;
using System.Runtime.CompilerServices;
using Ultros.Lists;

await Ultros.Lists.Smoke.ClientChecks.Run();
await Ultros.Lists.Smoke.ConnectionChecks.Run();
if(args is ["--client-only"])return;
if(args is ["--native-shadow",var shadowNative,var shadowDirectory]){Ultros.Lists.Smoke.NativeShadowChecks.Run(shadowNative,shadowDirectory);return;}
if(args is ["--http",var origin,var nativeDirectory]){Native.Configure(nativeDirectory);await Ultros.Lists.Smoke.HttpChecks.Run(origin);return;}

if (args.Length != 1) throw new ArgumentException("Pass the directory containing ultros_list_doc_ffi.dll.");
try { using var unavailable = Document.Open(); throw new Exception("Document opened before ABI verification."); }
catch (InvalidOperationException) { }
Native.Configure(args[0]);
await Ultros.Lists.Smoke.WorkerChecks.CartLifecycle();
Ultros.Lists.Smoke.CartCommandChecks.Run();
Ultros.Lists.Smoke.InventoryProgressChecks.NativeDeltas();
static void Require(bool condition, string message)
{
    if (!condition) throw new Exception(message);
}
using var a = Document.Open();
a.Edit(new { kind = "add", item_id = 8, quality = "nq", need = 10 });
using var b = Document.Open(a.Export());
Require(a.View.PeerId != b.View.PeerId, "Peers must have distinct identities.");
a.Edit(new { kind = "add_acquired", item_id = 8, quality = "nq", delta = 2 });
b.Edit(new { kind = "add_acquired", item_id = 8, quality = "nq", delta = 3 });
var fromB = b.Export();
for (var i = 0; i < 3; i++) a.Import(fromB);
b.Import(a.Export());
Require(a.View.Rows.Single().Acquired == 5, "Acquisitions must merge once despite duplicate frames.");
Require(a.View.Rows.SequenceEqual(b.View.Rows), "Both peers must converge.");
a.Edit(new { kind = "add_acquired", item_id = 8, quality = "nq", delta = 1 });
a.Edit(new { kind = "undo" });
Require(a.View.Rows.Single().Acquired == 5, "Undo must preserve remote purchases.");
a.Edit(new { kind = "redo" });
Require(a.View.Rows.Single().Acquired == 6, "Redo failed.");
using var restored = Document.Open(a.Export());
Require(restored.View.Rows.SequenceEqual(a.View.Rows), "Durable snapshot round trip failed.");
Require(restored.View.PeerId != a.View.PeerId, "Restoring must create a fresh writer identity.");
try { a.Import([1, 2, 3]); throw new Exception("Malformed import was accepted."); }
catch (InvalidOperationException) { }
a.Refresh();
Require(a.View.Rows.Single().Acquired == 6, "Rejected input changed live state.");
for (var i = 0; i < 256; i++)
{
    using var temporary = Document.Open();
    a.Refresh();
}

// SafeHandle also closes on the CLR finalizer thread. Exercise that real path,
// not just explicit Dispose on the UI/caller thread, beyond the 16-handle cap.
[MethodImpl(MethodImplOptions.NoInlining)]
static void OpenAndAbandon() => _ = Document.Open();
for (var batch = 0; batch < 8; batch++)
{
    for (var i = 0; i < 8; i++) OpenAndAbandon();
    GC.Collect();
    GC.WaitForPendingFinalizers();
    a.Refresh();
}

// Load the same pinned raw-Loro fixtures used by the browser compatibility tests.
using var fixtures = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "list-compatibility.json")));
byte[] Fixture(string name) => fixtures.RootElement.GetProperty(name).EnumerateArray().Select(value => value.GetByte()).ToArray();
using var compatible = Document.Open(Fixture("supported"));
Require(compatible.View.Name == "Compatibility fixture" && compatible.View.Rows.Single().ItemId == 5056, "Browser fixture was not compatible with the native bridge.");
foreach (var invalid in new[] { "future", "malformed" })
{
    try { using var rejected = Document.Open(Fixture(invalid)); throw new Exception("Incompatible fixture was accepted."); }
    catch (InvalidOperationException) { }
    try { compatible.Import(Fixture(invalid)); throw new Exception("Incompatible import was accepted."); }
    catch (InvalidOperationException) { }
    compatible.Refresh();
    Require(compatible.View.Rows.Single().Need == 5, "Rejected fixture changed the live document.");
}
a.Dispose();
try { a.Refresh(); throw new Exception("Closed handle was accepted."); }
catch (ObjectDisposedException) { }

var directory = Path.Combine(Path.GetTempPath(), "ultros-list-smoke-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(directory);
try
{
    using var store = new LocalCartStore(directory);
    store.OpenProfile("a");
    store.Edit(store.Main!, new { kind = "add", item_id = 8, quality = "nq", need = 10 });
    Require(!store.Dirty, "An edit should persist immediately.");
    var peerBeforeReload = store.Main!.View.PeerId;
    store.Reload();
    Require(store.Main!.View.Rows.Single().Need == 10, "Autosaved cart did not survive reopening.");
    Require(store.Main.View.PeerId != peerBeforeReload, "Reopening reused a live writer identity.");
    using var otherClient = new LocalCartStore(directory);
    try { otherClient.OpenProfile("a"); throw new Exception("Two clients acquired the same profile."); }
    catch (IOException) { }
    otherClient.OpenProfile("b");
    Require(otherClient.Main!.View.Rows.Length == 0, "Profiles must remain independent.");

    store.Fork();
    store.Edit(store.Main, new { kind = "add_acquired", item_id = 8, quality = "nq", delta = 2 });
    store.Edit(store.Comparison!, new { kind = "add_acquired", item_id = 8, quality = "nq", delta = 3 });
    store.MergeIntoMain();
    store.MergeIntoMain();
    store.Reload();
    Require(store.Comparison is null, "Scratch peer should be cleared on reopen.");
    Require(store.Main!.View.Rows.Single().Acquired == 5, "Merged purchases were not saved exactly once.");

    var corrupt = Path.Combine(directory, "corrupt");
    Directory.CreateDirectory(corrupt);
    var corruptFile = Path.Combine(corrupt, "cart.loro");
    File.WriteAllBytes(corruptFile, [1, 2, 3]);
    try { store.OpenProfile("corrupt"); throw new Exception("Corrupt data was accepted."); }
    catch (InvalidOperationException) { }
    Require(store.Profile == "a" && store.Main.View.Rows.Single().Acquired == 5, "Failed profile load discarded the open cart.");
    Require(File.ReadAllBytes(corruptFile).SequenceEqual(new byte[] { 1, 2, 3 }), "Corrupt snapshot was overwritten.");

    // Make replacement fail without relying on OS permissions: a directory occupies
    // the snapshot path. The changed document must remain available for a retry.
    var snapshotFile = Path.Combine(directory, "a", "cart.loro");
    File.Delete(snapshotFile);
    Directory.CreateDirectory(snapshotFile);
    try { store.Edit(store.Main, new { kind = "add_acquired", item_id = 8, quality = "nq", delta = 1 }); throw new Exception("Save should have failed."); }
    catch (IOException) { }
    catch (UnauthorizedAccessException) { }
    Require(store.Dirty && store.Main.View.Rows.Single().Acquired == 6, "Failed save lost unsaved work.");
    try { store.OpenProfile("new"); throw new Exception("Unsaved work was discarded by a profile switch."); }
    catch (InvalidOperationException) { }
    Directory.Delete(snapshotFile);
    store.Save();
    store.Reload();
    Require(!store.Dirty && store.Main!.View.Rows.Single().Acquired == 6, "Retry did not save unsaved work.");
}
finally
{
    // Only remove files/directories created above, without a recursive delete.
    foreach (var profileDirectory in Directory.EnumerateDirectories(directory))
    {
        foreach (var file in Directory.EnumerateFiles(profileDirectory)) File.Delete(file);
        var blockedSave = Path.Combine(profileDirectory, "cart.loro");
        if (Directory.Exists(blockedSave)) Directory.Delete(blockedSave);
        Directory.Delete(profileDirectory);
    }
    Directory.Delete(directory);
}
Console.WriteLine("PASS: C# -> Rust ABI; browser compatibility fixtures; concurrent peers; duplicate imports; undo/redo; explicit/finalizer handle cleanup; local autosave/reload; profile locks; corrupt-file preservation; failed-save retry.");
