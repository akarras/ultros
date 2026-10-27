using System.Collections.Concurrent;
using System.Text.Json;
using System.Threading.Channels;
using Ultros.Lists;
namespace Ultros.Client;

public sealed record CartEstimate(string Version, string Scope, DateTimeOffset ObservedAt, long Total, bool Incomplete, long UnpricedUnits, EstimateLine[] Lines);
public sealed record EstimateLine(int ItemId, bool? Hq, int Remaining, int PricedUnits, long Total, string Status);
public sealed record ClientView(bool Connected, string Status, string StorageMode, string? HumanCode, string VerificationUrl,
    DeviceSession? Session, ListAccess[] Lists, int? SelectedList, CartView? Cart, bool CanEdit, UnverifiedCharacter[] Characters,
    GameDelivery[] Inbox, MarketView? Market, SaleRow[] Sales, CartEstimate? Estimate, string[] Scopes, string PriceRegion = "", string NotificationTransport = "Polling", string RosterStatus = "", bool Registering = false, string? ApprovalUrl = null,
    RetainerPriceLookup? RetainerPrices = null, string RetainerPriceStatus = "", Guid? CartInstance = null, long RecoveryRevision = 0, string[]? ReadNotificationIds = null,
    RetainerSnapshot[]? RetainerSnapshots = null, bool RefreshingRetainers = false)
{
    public override string ToString() => $"ClientView {{ Connected = {Connected}, pairing credentials redacted }}";
}

/// <summary>One background owner of HTTP, disk and native documents. UI submits bounded commands.</summary>
public sealed class ClientWorker : IDisposable
{
    private readonly Channel<Func<Task>> commands = Channel.CreateBounded<Func<Task>>(64);
    private readonly ConcurrentQueue<GameDelivery> toasts = new();
    private readonly CancellationTokenSource stop = new();
    private readonly Task running;
    private readonly string root;
    private readonly UltrosEndpoint endpoint;
    private readonly string profile;
    private Connection? connection;
    private ConnectedCart? cart;
    private ListAccess[] lists = [];
    private UnverifiedCharacter[] characters = [];
    private RetainerSnapshot[]? retainerSnapshots;
    private bool refreshingRetainers;
    private NotificationHistory? history;
    private readonly HashSet<string> queuedToasts = [];
    private MarketView? market;
    private SaleRow[] sales = [];
    private CartEstimate? estimate;
    private WorldDirectory? worlds;
    private string priceRegion="";
    private NotificationFeed? notificationFeed;
    private DateTimeOffset nextFeed;
    private int notificationWake,feedReady;
    private string status = "Starting…";
    private string rosterStatus = "";
    private RetainerPriceLookup? retainerPrices;
    private string retainerPriceStatus="";
    private string? pricesDevice,pricesAccount;
    private bool registering;
    private DateTimeOffset nextNotifications, nextLists, nextCart;
    private string? loadedDevice;
    private int queuedCaptures;
    private int captureGeneration;
    public void CancelQueuedCaptures()=>Interlocked.Increment(ref captureGeneration);
    private int inventoryGeneration;
    private int inventoryBatchReserved;
    private PendingInventory? pendingInventory;
    private DateTimeOffset nextInventory;
    private sealed record PendingInventory(ListEditTarget Target, int Generation, InventoryBatch Batch);
    public int InventoryGeneration=>Volatile.Read(ref inventoryGeneration);
    public void CancelQueuedInventory()=>Interlocked.Increment(ref inventoryGeneration);
    private ClientView view;
    public ClientView View => Volatile.Read(ref view);
    public ClientWorker(string root, UltrosEndpoint endpoint, string profile)
    {
        this.root=root;this.endpoint=endpoint;this.profile=profile;
        view=new(false,status,"",null,endpoint.At("/plugin/verify").AbsoluteUri,null,[],null,null,false,[],[],null,[],null,[]);
        running=Task.Run(Run);
    }
    private bool Offer(Func<Task> action) => commands.Writer.TryWrite(action);
    public bool Pair(string label, bool editLists=true, bool register=true, bool upload=true, bool readRetainers=true)
    {
        CancelQueuedInventory();
        return Offer(async () => {
        var scopes=new List<string>(Protocol.DefaultScopes);
        if(!editLists)scopes.Remove("lists:write");
        if(!readRetainers&&!register&&!upload)scopes.Remove("retainers:read");
        if(!register)scopes.Remove("retainers:register");
        if(!upload)scopes.Remove("retainers:upload");
        await connection!.BeginPairing(label,scopes.ToArray(),stop.Token);
        });
    }
    public bool CancelPairing()=>Offer(()=>{connection!.CancelPairing();return Task.CompletedTask;});
    public bool Disconnect(){CancelQueuedInventory();return Offer(async()=>{
        try{CloseCart();}
        catch(Exception){status="Disconnect paused: unsaved list edits could not be saved. Retry saving, then disconnect. Your connection and edits are still available.";return;}
        await StopFeed();
        await connection!.Disconnect();lists=[];characters=[];history=null;loadedDevice=null;estimate=null;rosterStatus="";
        ClearRetainerPrices();
        while(toasts.TryDequeue(out _)){}queuedToasts.Clear();CancelQueuedCaptures();
        status="Disconnected. Saved cart work remains bound to its original account and server.";
    });}
    public bool Refresh()=>Offer(async()=>{await connection!.Restore(stop.Token,true);await RefreshAccount();nextNotifications=DateTimeOffset.MinValue;nextCart=DateTimeOffset.MinValue;});
    public bool SelectList(int id){CancelQueuedInventory();return Offer(()=>OpenList(id));}
    private void CloseCart()
    {
        // Keep the live document if its checkpoint cannot be written. This also
        // fences a new device/account from replacing unsaved work during pairing.
        if(cart?.Document.View.AwaitingPersist==true)cart.Save();
        cart?.Dispose();cart=null;
    }
    private bool SessionReady()=>connection?.Connected==true&&loadedDevice==connection.Session?.Id;
    private async Task OpenList(int id)
    {
        CancelQueuedInventory();
        if(!lists.Any(l=>l.List.Id==id))throw new InvalidOperationException("Refresh the available lists first.");
        CloseCart();estimate=null;
        cart=new ConnectedCart(connection!,id);await SyncCart();nextCart=DateTimeOffset.UtcNow.AddSeconds(60);
        AtomicFile.Write(SelectionPath(),JsonSerializer.SerializeToUtf8Bytes(id));
    }
    private string SelectionPath()=>Path.Combine(connection!.Directory,"accounts",connection.Session!.AccountId,"selected-list.json");
    public bool SetCurrentWorld(int current,int home)=>Offer(async()=>{
        worlds??=await connection!.Public<WorldDirectory>("/api/v1/world_data",stop.Token);
        priceRegion=worlds.RegionFor(current,home);
    });
    private bool MatchesDestination(ListEditTarget? target) => connection?.Connected==true
        &&target?.MatchesDestination(connection.Session,cart?.Id,cart?.InstanceId)==true;
    private bool RejectChangedDestination()
    {
        status="The list or account changed. Choose the destination and try again.";
        return false;
    }
    public bool Edit(CartCommandTarget? target,object edit)=>target is not null&&Offer(()=>{
        if(!CartCommands.Edit(target,connection?.Session,cart,CanEdit(),edit)){RejectChangedDestination();return Task.CompletedTask;}
        estimate=null;nextCart=DateTimeOffset.MinValue;status="Saved locally; synchronizing…";return Task.CompletedTask;
    });
    public bool AddItem(ListEditTarget target,int item,string quality="any",int quantity=1)
    {
        if(!target.Matches(View)||item<=0||quantity is <1 or >1_000_000||quality is not ("any" or "nq" or "hq"))return false;
        return Offer(()=>{
            if(!target.Matches(connection?.Session,cart?.Id,CanEdit(),cart?.InstanceId)) {
                status="The list or account changed. Choose the destination and add the item again.";
                return Task.CompletedTask;
            }
            cart!.Edit(new{kind="add",item_id=item,quality,need=quantity});
            estimate=null;nextCart=DateTimeOffset.MinValue;
            status="Item added to the selected list. Saved locally; synchronizing…";
            return Task.CompletedTask;
        });
    }
    public bool ApplyInventoryGains(ListEditTarget target,int generation,InventoryGain[] gains)
    {
        if(generation!=InventoryGeneration||!target.Matches(View)||gains.Length>4096
            ||Interlocked.CompareExchange(ref inventoryBatchReserved,1,0)!=0)return false;
        var gainsCopy=gains.ToArray();
        var accepted=Offer(()=>{
            if(generation!=InventoryGeneration||!MatchesDestination(target)||!CanTrackInventory()) {
                Interlocked.Exchange(ref inventoryBatchReserved,0);return Task.CompletedTask;
            }
            pendingInventory=new(target,generation,new InventoryBatch(InventoryProgress.Allocate(cart!.Document.View.Rows,gainsCopy)));
            ResumeInventory();
            return Task.CompletedTask;
        });
        if(!accepted)Interlocked.Exchange(ref inventoryBatchReserved,0);
        return accepted;
    }
    private void ResumeInventory()
    {
        if(pendingInventory is not { } pending)return;
        if(pending.Generation!=InventoryGeneration||!MatchesDestination(pending.Target)) {
            pendingInventory=null;Interlocked.Exchange(ref inventoryBatchReserved,0);return;
        }
        var activeCart=cart!;
        if(activeCart.Document.View.RecoveryReview)return;
        nextInventory=DateTimeOffset.UtcNow.AddSeconds(15);
        try {
            // A previously queued manual edit may also have failed to save.
            if(activeCart.Document.View.AwaitingPersist)activeCart.Save();
            // Persist an already-mutated delta before attempting any later delta.
            pending.Batch.Resume(
                delta=>activeCart.Document.Edit(new{kind="add_acquired",item_id=delta.ItemId,quality=delta.Quality,delta=delta.Quantity}),
                ()=>activeCart.Save(),
                ()=>pending.Generation==InventoryGeneration&&MatchesDestination(pending.Target)&&CanEdit(),
                delta=>InventoryProgress.ClampToRemaining(activeCart.Document.View.Rows,delta));
            estimate=null;nextCart=DateTimeOffset.MinValue;
            status=$"Marked {pending.Batch.Applied:N0} units acquired from inventory. Saved locally; synchronizing...";
            if(pending.Batch.Complete){pendingInventory=null;Interlocked.Exchange(ref inventoryBatchReserved,0);}
        }
        catch(Exception) when(pending.Batch.AwaitingPersist||activeCart.Document.View.AwaitingPersist) {
            estimate=null;
            status=$"Inventory progress paused after {pending.Batch.Applied:N0} units; retry saving to finish the retained batch.";
        }
        catch(Exception) {
            // Native admission can require a sync/ack before the next edit fits.
            // Keep the unmutated cursor and let the rest of the worker loop run.
            status="Inventory progress is waiting for cart synchronization. Remaining gains are retained.";
            nextCart=DateTimeOffset.MinValue;
        }
    }
    public bool RetrySave(CartCommandTarget? target)=>target is not null&&Offer(()=>{
        if(connection?.Connected!=true||target?.Matches(connection?.Session,cart)!=true){RejectChangedDestination();return Task.CompletedTask;}
        cart!.Save();ResumeInventory();nextCart=DateTimeOffset.MinValue;return Task.CompletedTask;
    });
    private bool Recover(RecoveryChoiceTarget? target,bool useServer)=>target is not null&&Offer(()=>{
        if(connection?.Connected==true&&target.Matches(connection.Session,cart)&&(!useServer||cart!.Document.View.HasServerCopy)) {
            // A server-copy choice supersedes pending inventory for the discarded document.
            if(useServer)CancelQueuedInventory();
            CartCommands.Recover(target,connection?.Session,cart,useServer);
            nextCart=DateTimeOffset.MinValue;
        }
        else status="The recovery choices changed. Review the current copies and choose again.";
        return Task.CompletedTask;
    });
    public bool UseServerCopy(RecoveryChoiceTarget? target)=>Recover(target,true);
    public bool RetryRecovery(RecoveryChoiceTarget? target)=>Recover(target,false);
    public bool Estimate(CartCommandTarget? target)=>target is not null&&Offer(async()=>{
        if(connection?.Connected!=true||target?.Matches(connection?.Session,cart)!=true){RejectChangedDestination();return;}
        await SyncCart();
        if(!target.Matches(connection?.Session,cart))return;
        estimate=await connection!.Send<CartEstimate>(HttpMethod.Get,$"/api/v1/plugin/lists/{cart!.Id}/estimate",null,stop.Token);
        status="Market estimate refreshed. Prices are not reserved; whole stacks may cost more.";
    });
    public bool Observe(ObservedCharacter observation)=>Offer(async()=>{
        characters=await connection!.Send<UnverifiedCharacter[]>(HttpMethod.Post,"/api/v1/plugin/characters/observe",observation,stop.Token);
        rosterStatus=status="Character saved as an unverified game observation. Save its retainers to add them to your account.";
    });
    internal static UnverifiedCharacter? LatestObservation(IEnumerable<UnverifiedCharacter> roster,ObservedCharacter observation)=>
        roster.Where(c=>c.Name==observation.Name&&c.HomeWorld==observation.HomeWorld)
            .OrderByDescending(c=>c.ObservedAt).FirstOrDefault();
    public static bool NeedsRegistration(IEnumerable<UnverifiedCharacter> roster,ObservedCharacter observation)
    {
        var saved=LatestObservation(roster,observation);
        return observation.Retainers.Length>0&&(saved?.Registered!=true||!saved.UploadEnabled&&saved.CaptureEpoch==0||observation.Retainers.Any(observed=>
            !saved.Retainers.Any(retainer=>retainer.Name==observed.Name&&(retainer.Registered||retainer.GroupingConflict))));
    }
    public bool Register(ObservedCharacter observation)
    {
        // A queued observation belongs to the account/device that saw it.
        var session=View.Session;
        if(!View.Connected||session is null||!session.Scopes.Contains("retainers:register"))return false;
        return Offer(async()=>{
            if(connection?.Connected!=true||connection.Session?.Id!=session.Id
                ||connection.Session.AccountId!=session.AccountId||!connection.HasScope("retainers:register"))return;
            registering=true;rosterStatus="Saving retainers to your Ultros account…";Publish();
            try {
                var request=new {operation_id=Guid.NewGuid().ToString("N"),character=observation};
                characters=await connection!.Send<UnverifiedCharacter[]>(HttpMethod.Post,"/api/v1/plugin/retainers/register",request,stop.Token);
                var saved=LatestObservation(characters,observation);
                var count=observation.Retainers.Count(observed=>saved?.Retainers.Any(r=>r.Registered&&r.Name==observed.Name)==true);
                rosterStatus=status=$"Saved {count} of {observation.Retainers.Length} retainers for {observation.Name}. Available on the website and in game."
                    +(count<observation.Retainers.Length?" Some retainers already belong to another saved character; their assignments were kept.":"");
            }
            catch(ApiFailure e){rosterStatus="Retainers were not confirmed saved. "+e.Message;throw;}
            catch(Exception e){rosterStatus="Retainers were not confirmed saved. "+SafeError(e);throw;}
            finally{registering=false;}
        });
    }
    public bool UploadConsent(string character,bool enabled,bool delete=false)=>Offer(async()=>{
        characters=await connection!.Send<UnverifiedCharacter[]>(HttpMethod.Post,$"/api/v1/plugin/characters/{PositiveId(character)}/uploads",new{enabled,delete_snapshots=delete},stop.Token);
        if(delete){var ids=characters.Where(c=>c.Id==character).SelectMany(c=>c.Retainers).Select(r=>r.Id).ToHashSet();retainerSnapshots=retainerSnapshots?.Where(s=>!ids.Contains(s.RetainerId)).ToArray();}
        status=enabled?"Uploads enabled for this character. Revisit a retainer's selling list.":"Uploads paused. Queued captures from the previous consent epoch are invalid.";
    });
    public Task<CaptureUploadResult>? Upload(string retainer,ListingCapture capture)
    {
        if(Interlocked.Increment(ref queuedCaptures)>10){Interlocked.Decrement(ref queuedCaptures);return null;}
        var session=View.Session;
        var generation=Volatile.Read(ref captureGeneration);
        var completion=new TaskCompletionSource<CaptureUploadResult>(TaskCreationOptions.RunContinuationsAsynchronously);
        var accepted=Offer(async()=>{try{
            if(generation!=Volatile.Read(ref captureGeneration)||session is null||!connection!.Connected
                ||connection.Session?.AccountId!=session.AccountId||connection.Session?.Id!=session.Id
                ||DateTimeOffset.UtcNow-capture.ObservedAt>TimeSpan.FromMinutes(10))return;
            var receipt=await connection.Send<CaptureReceipt>(HttpMethod.Post,$"/api/v1/plugin/retainers/{PositiveId(retainer)}/listings",capture,stop.Token);
            // The POST is acknowledged even if the following roster read fails.
            // Publish its revision before the game thread can queue the next capture.
            characters=characters.Select(c=>c with{Retainers=c.Retainers.Select(r=>r.Id==retainer?r with{Revision=Math.Max(r.Revision,receipt.Revision)}:r).ToArray()}).ToArray();
            if(retainerSnapshots?.FirstOrDefault(s=>s.RetainerId==retainer) is not { } known||known.Revision<=receipt.Revision)
                retainerSnapshots=[..(retainerSnapshots??[]).Where(s=>s.RetainerId!=retainer),new(retainer,receipt.Revision,DateTimeOffset.UtcNow,capture.ObservedAt,capture.Listings)];
            status="Private retainer listing snapshot uploaded.";
            try{characters=await connection.Send<UnverifiedCharacter[]>(HttpMethod.Get,"/api/v1/plugin/characters",null,stop.Token);}
            catch(Exception)when(!stop.IsCancellationRequested){nextLists=DateTimeOffset.MinValue;}
            Publish();completion.TrySetResult(new(true));
        }
        catch(OperationCanceledException)when(stop.IsCancellationRequested){throw;}
        catch(Exception error){
            status=error is ApiFailure api?api.Message:SafeError(error);
            var retry=error is HttpRequestException or OperationCanceledException||error is ApiFailure{Status:429 or >=500};
            completion.TrySetResult(new(false,retry,error is ApiFailure failure?Math.Max(15,failure.RetryAfter):15));
        }
        finally{Interlocked.Decrement(ref queuedCaptures);completion.TrySetResult(new(false));}});
        if(!accepted)Interlocked.Decrement(ref queuedCaptures);
        return accepted?completion.Task:null;
    }
    public bool TestNotification()=>Offer(async()=>{
        _=await connection!.Send<bool>(HttpMethod.Post,"/api/v1/plugin/notifications/test",new{operation_id=Guid.NewGuid().ToString("N")},stop.Token);
        nextNotifications=DateTimeOffset.MinValue;
    });
    private bool SameNotificationSession(DeviceSession? expected)=>expected is not null&&SessionReady()
        &&connection!.Session?.Id==expected.Id&&connection.Session.AccountId==expected.AccountId;
    public bool MarkRead(string id)
    {
        var expected=View.Session;
        return Offer(async()=>{
            if(!SameNotificationSession(expected)||history is null)return;
            var entry=history.Entries.FirstOrDefault(d=>d.Id==id);
            if(entry is null)return;
            // Test deliveries have no website notification to mark read.
            if(entry.EventId is not null)
                _=await connection!.Send<bool>(HttpMethod.Post,"/api/v1/plugin/notifications/read",new{ids=new[]{PositiveId(id)}},stop.Token);
            history.MarkRead(id);status="Notification marked read.";
        });
    }
    public bool DeleteNotifications(DeviceSession? expected,string[] ids)=>Offer(()=>{
        if(SameNotificationSession(expected)&&history is not null){history.Delete(ids,DateTimeOffset.UtcNow);nextNotifications=DateTimeOffset.MinValue;status="Removed from this client's history.";}
        return Task.CompletedTask;
    });
    public bool ToastShown(string id)
    {
        var expected=View.Session;
        return Offer(async()=>{
            if(!SameNotificationSession(expected)||history is null)return;
            history.MarkShown(id);
            _=await connection!.Send<bool>(HttpMethod.Post,"/api/v1/plugin/notifications/ack",new{ids=new[]{id}},stop.Token);
        });
    }
    public bool TryToast(out GameDelivery? delivery)
    {
        while(toasts.TryDequeue(out delivery))
            if(Volatile.Read(ref history) is { } current&&!current.IsDeleted(delivery.Id))return true;
        delivery=null;return false;
    }
    private void ClearRetainerPrices(){retainerPrices=null;retainerPriceStatus="";pricesDevice=pricesAccount=null;retainerSnapshots=null;}
    public bool RefreshRetainers()
    {
        var expected=View.Session;
        return Offer(async()=>{
            if(expected is null||!SessionReady()||connection!.Session?.Id!=expected.Id||connection.Session.AccountId!=expected.AccountId||!connection.HasScope("retainers:read"))return;
            refreshingRetainers=true;Publish();
            try{await ReadRetainers();status="Retainer listings refreshed.";}
            finally{refreshingRetainers=false;}
        });
    }
    private async Task ReadRetainers()
    {
        characters=await connection!.Send<UnverifiedCharacter[]>(HttpMethod.Get,"/api/v1/plugin/characters",null,stop.Token);
        // Preserve the existing rows while refreshing; the table never clears
        // or changes columns as each retainer loads.
        retainerSnapshots=await connection.Send<RetainerSnapshot[]>(HttpMethod.Get,"/api/v1/plugin/retainers/snapshots",null,stop.Token);
    }
    public bool LookupRetainer(string retainer)
    {
        var session=View.Session;
        if(!View.Connected||session is null||!session.Scopes.Contains("retainers:read"))return false;
        return Offer(async()=>{
            if(connection?.Connected!=true||connection.Session?.Id!=session.Id
                ||connection.Session.AccountId!=session.AccountId||!connection.HasScope("retainers:read"))return;
            retainerPrices=null;pricesDevice=session.Id;pricesAccount=session.AccountId;
            retainerPriceStatus="Loading this retainer's captured listings and same-world comparisons…";Publish();
            try {
                retainerPrices=await connection.Send<RetainerPriceLookup>(HttpMethod.Get,$"/api/v1/plugin/retainers/{PositiveId(retainer)}/prices",null,stop.Token);
                retainerPriceStatus="Suggestions use cached prices for the same quality on this retainer's world. Your saved retainers are excluded.";
            }
            catch(ApiFailure e){
                retainerPriceStatus=e.Code=="retainer_registration_required"
                    ? "Save this retainer to your Ultros account, then refresh. Its website assignment may have changed."
                    : e.Status==429?"Too many refreshes. Wait a minute, then retry.":e.Message;
                throw;
            }
            catch(Exception e){retainerPriceStatus=SafeError(e);throw;}
        });
    }
    public bool FetchItem(int item,string scope)=>Offer(async()=>{
        if(item<=0||scope.Length is <1 or >64)throw new ArgumentException("Select an item and price zone.");
        using var response=await connection!.Public<JsonDocument>($"/api/v1/listings/{Uri.EscapeDataString(scope)}/{item}",stop.Token);
        var rows=response.RootElement.GetProperty("listings").EnumerateArray().Select(pair=>{
            var listing=pair[0];var retainer=pair[1];
            return new MarketRow(listing.GetProperty("world_id").GetInt32().ToString(),retainer.GetProperty("name").GetString()??"Retainer",
                listing.GetProperty("hq").GetBoolean(),listing.GetProperty("quantity").GetInt32(),listing.GetProperty("price_per_unit").GetInt64());
        }).OrderBy(l=>l.UnitPrice).Take(100).ToArray();
        sales=response.RootElement.GetProperty("sales").EnumerateArray().Take(30).Select(s=>new SaleRow(s.GetProperty("hq").GetBoolean(),s.GetProperty("quantity").GetInt32(),s.GetProperty("price_per_item").GetInt64(),s.GetProperty("sold_date").GetString()??"")).ToArray();
        market=new(item,scope,rows,$"Fetched {DateTimeOffset.Now:t} · cached market data");status="Item market loaded.";
    });

    private static string PositiveId(string id)=>long.TryParse(id,out var value)&&value>0&&value.ToString()==id?id:throw new ArgumentException("Invalid identifier.");
    private bool CanTrackInventory()=>SessionReady()&&connection!.HasScope("lists:write")&&cart is not null
        &&!cart.AccessDenied&&lists.Any(list=>list.List.Id==cart.Id&&list.CanWrite)
        &&cart.Document.View.Ready&&!cart.Document.View.RecoveryReview;
    private bool CanEdit()=>SessionReady()&&connection!.HasScope("lists:write")&&cart is not null
        &&!cart.AccessDenied
        &&lists.Any(l=>l.List.Id==cart.Id&&l.CanWrite)&&cart.Document.View.Ready&&!cart.Document.View.RecoveryReview&&!cart.Document.View.AwaitingPersist;
    private async Task SyncCart()
    {
        if(cart is null)return;
        try {cart.CanWrite=lists.Any(l=>l.List.Id==cart.Id&&l.CanWrite);await cart.Sync(connection!,stop.Token);status=cart.Document.View.RecoveryReview?"Recovery needs review. Both copies are preserved.":"Cart synchronized.";}
        catch(Exception)when(cart?.AccessDenied==true){
            cart.Dispose();cart=null;estimate=null;lists=[];throw;
        }
    }
    private async Task RefreshAccount()
    {
        if(connection?.Connected!=true)return;
        if(connection.HasScope("lists:read"))lists=await connection.Send<ListAccess[]>(HttpMethod.Get,"/api/v1/plugin/lists",null,stop.Token);
        if(cart is null)
        {
            int? remembered=null;
            try{var path=SelectionPath();if(File.Exists(path)&&new FileInfo(path).Length<=32)remembered=JsonSerializer.Deserialize<int>(File.ReadAllBytes(path));}catch(JsonException){}catch(IOException){}
            if(ListSelection.Default(lists,remembered,connection.Session!.AccountId) is { } initial)await OpenList(initial);
        }
        if(connection.HasScope("retainers:read"))await ReadRetainers();
        nextLists=DateTimeOffset.UtcNow.AddSeconds(60);
    }
    private void LoadInbox()
    {
        history=null;queuedToasts.Clear();
        while(toasts.TryDequeue(out _)){}
        var session=connection!.Session!;
        loadedDevice=session.Id;
        var dir=Path.Combine(connection.Directory,"accounts",session.AccountId,"notifications");
        Directory.CreateDirectory(dir);
        var path=Path.Combine(dir,session.Id+".json");
        try{history=new NotificationHistory(path);}
        catch(JsonException){status="Local notification history could not be read. The original file has been preserved.";return;}
        foreach(var item in history.Entries.Where(d=>d.ExpiresAt>DateTimeOffset.UtcNow&&!history.WasShown(d.Id))){toasts.Enqueue(item);queuedToasts.Add(item.Id);}
    }
    private async Task PollNotifications()
    {
        if(history is null){nextNotifications=DateTimeOffset.UtcNow.AddMinutes(1);return;}
        var batch=await connection!.Send<DeliveryBatch>(HttpMethod.Get,"/api/v1/plugin/notifications",null,stop.Token);
        history.Receive(batch.Deliveries,DateTimeOffset.UtcNow);
        foreach(var item in batch.Deliveries)
        {
            if(item.ExpiresAt<=DateTimeOffset.UtcNow||history.WasShown(item.Id)||history.IsDeleted(item.Id)||!queuedToasts.Add(item.Id))continue;
            toasts.Enqueue(item);
        }
        var retry=batch.Deliveries.Where(i=>history.WasShown(i.Id)||history.IsDeleted(i.Id)).Select(i=>i.Id).ToArray();
        if(retry.Length>0)_=await connection.Send<bool>(HttpMethod.Post,"/api/v1/plugin/notifications/ack",new{ids=retry},stop.Token);
        nextNotifications=DateTimeOffset.UtcNow.AddMilliseconds(Math.Clamp(batch.PollAfterMs,1000,60_000));
    }
    private async Task StopFeed()
    {
        if(notificationFeed is not null)
        {
            notificationFeed.Dispose();
            try{await notificationFeed.Completion;}catch(Exception){}
            notificationFeed=null;
        }
        nextFeed=DateTimeOffset.MinValue;Interlocked.Exchange(ref notificationWake,0);Interlocked.Exchange(ref feedReady,0);
    }
    private async Task UpdateFeed()
    {
        if(notificationFeed?.Completion.IsCompleted==true)
        {
            await StopFeed();nextFeed=DateTimeOffset.UtcNow.AddSeconds(5);
        }
        if(notificationFeed is null&&DateTimeOffset.UtcNow>=nextFeed)
        {
            notificationFeed=await connection!.SubscribeNotifications(()=>{Interlocked.Exchange(ref notificationWake,1);Interlocked.Exchange(ref feedReady,1);},stop.Token);
            nextFeed=DateTimeOffset.UtcNow.AddSeconds(30);
        }
        if(Interlocked.Exchange(ref notificationWake,0)!=0)nextNotifications=DateTimeOffset.MinValue;
    }
    private void Publish()
    {
        var connected=connection?.Connected==true;
        var ready=SessionReady();
        var visibleCart=ready?cart:null;
        var showPrices=connected&&connection!.HasScope("retainers:read")
            &&pricesDevice==connection.Session?.Id&&pricesAccount==connection.Session?.AccountId;
        var visibleStatus=pendingInventory is { } pending&&cart?.Document.View.AwaitingPersist==true
            ? $"Inventory progress paused after {pending.Batch.Applied:N0} units; retry saving to finish the retained batch."
            : status;
        Volatile.Write(ref view,new ClientView(connected,visibleStatus,connection?.StorageMode??"",connection?.HumanCode,connection?.VerificationUrl??endpoint.At("/plugin/verify").AbsoluteUri,
            connection?.Session,ready?lists:[],visibleCart?.Id,visibleCart?.Document.View,CanEdit(),ready?characters:[],ready?history?.Entries??[]:[],market,sales,ready?estimate:null,connection?.Session?.Scopes??[],priceRegion,Volatile.Read(ref feedReady)!=0?"Live notifications (SSE)":"Polling fallback",ready?rosterStatus:"",registering,connection?.ApprovalUrl,
            showPrices?retainerPrices:null,showPrices?retainerPriceStatus:"",visibleCart?.InstanceId,visibleCart?.RecoveryRevision??0,ready?history?.ReadIds:[],
            ready&&connection!.HasScope("retainers:read")?retainerSnapshots:null,ready&&refreshingRetainers));
    }
    private async Task<Connection> AcquireConnection()
    {
        for(var attempt=0;;attempt++) {
            stop.Token.ThrowIfCancellationRequested();
            try{return new Connection(root,endpoint,profile);}
            catch(IOException) when(attempt<20) {
                status="Waiting for the previous Ultros instance to finish closing...";Publish();
                await Task.Delay(500,stop.Token);
            }
        }
    }
    private async Task Run()
    {
        try
        {
            connection=await AcquireConnection();
            await connection.Initialize(stop.Token);status=connection.Status;Publish();
            while(!stop.IsCancellationRequested)
            {
                while(commands.Reader.TryRead(out var command))
                {
                    try{await command();}
                    catch(OperationCanceledException)when(stop.IsCancellationRequested){throw;}
                    catch(ApiFailure e){status=e.Message;if(e.Status==401){characters=[];lists=[];ClearRetainerPrices();}}
                    catch(Exception e){status=SafeError(e);}
                    Publish();
                }
                try
                {
                    if(pendingInventory is not null&&(pendingInventory.Generation!=InventoryGeneration||DateTimeOffset.UtcNow>=nextInventory))ResumeInventory();
                    if(await connection.PollPairing(stop.Token)){status=connection.Status;nextLists=nextNotifications=nextCart=DateTimeOffset.MinValue;}
                    if(await connection.Restore(stop.Token)){status=connection.Status;nextLists=nextNotifications=nextCart=DateTimeOffset.MinValue;}
                    if(connection.Connected)
                    {
                        if(loadedDevice!=connection.Session!.Id){
                            CancelQueuedInventory();await StopFeed();
                            // A failed save can delay the transition. Stop displaying
                            // the previous device's queued messages during that pause.
                            while(toasts.TryDequeue(out _)){}queuedToasts.Clear();
                            CloseCart();estimate=null;rosterStatus="";ClearRetainerPrices();LoadInbox();nextLists=nextNotifications=DateTimeOffset.MinValue;
                        }
                        if(DateTimeOffset.UtcNow>=nextLists)await RefreshAccount();
                        if(connection.HasScope("notifications:read"))await UpdateFeed();
                        if(DateTimeOffset.UtcNow>=nextNotifications&&connection.HasScope("notifications:read"))await PollNotifications();
                        if(DateTimeOffset.UtcNow>=nextCart&&cart is not null){await SyncCart();nextCart=DateTimeOffset.UtcNow.AddSeconds(60);}
                    }
                    else {
                        if(connection.CanRestore)status=connection.Status;
                        if(notificationFeed is not null)await StopFeed();
                    }
                }
                catch(OperationCanceledException)when(stop.IsCancellationRequested){throw;}
                catch(Exception e){status=e is ApiFailure a?a.Message:SafeError(e);nextLists=nextNotifications=nextCart=DateTimeOffset.UtcNow.AddSeconds(15);}
                Publish();await Task.Delay(250,stop.Token);
            }
        }
        catch(OperationCanceledException)when(stop.IsCancellationRequested){}
        catch(Exception e){status=SafeError(e);Publish();}
        finally
        {
            await StopFeed();
            try{if(cart?.Document.View.AwaitingPersist==true)cart.Save();}catch{status="Final cart save failed; the last checkpoint is preserved.";}
            cart?.Dispose();cart=null;
            if(connection is not null){await connection.RevokeMemoryOnlyOnExit();connection.Dispose();}
        }
    }
    // Never expose exception messages from HTTP, JSON or credential APIs; they can contain secrets.
    private static string SafeError(Exception e)=>e switch{
        InvalidOperationException=>"The operation could not complete. Retry saving or reconnect; existing cart files are preserved.",
        IOException=>"Local storage is unavailable or this profile is open in another game process. Existing files are preserved.",
        HttpRequestException=>"Ultros could not be reached. Local cart work is preserved.",
        _=>"The operation could not complete. Reconnect or refresh to retry."
    };
    public void Dispose()
    {
        CancelQueuedInventory();commands.Writer.TryComplete();stop.Cancel();
        try{running.Wait(TimeSpan.FromSeconds(5));}catch(AggregateException){}
        // The worker retains ownership until its finally block completes.
    }
}
