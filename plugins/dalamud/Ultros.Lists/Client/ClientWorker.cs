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
    RetainerPriceLookup? RetainerPrices = null, string RetainerPriceStatus = "")
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
    private readonly List<GameDelivery> inbox = [];
    private readonly HashSet<string> shown = [];
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
    private string? inboxPath;
    private int queuedCaptures;
    private int captureGeneration;
    public void CancelQueuedCaptures()=>Interlocked.Increment(ref captureGeneration);
    private int inventoryGeneration;
    public int InventoryGeneration=>Volatile.Read(ref inventoryGeneration);
    public void CancelQueuedInventory()=>Interlocked.Increment(ref inventoryGeneration);
    private ClientView view;
    public ClientView View => Volatile.Read(ref view);
    private sealed record SavedInbox(GameDelivery[] Entries, string[] Shown);
    public ClientWorker(string root, UltrosEndpoint endpoint, string profile)
    {
        this.root=root;this.endpoint=endpoint;this.profile=profile;
        view=new(false,status,"",null,endpoint.At("/plugin/verify").AbsoluteUri,null,[],null,null,false,[],[],null,[],null,[]);
        running=Task.Run(Run);
    }
    private bool Offer(Func<Task> action) => commands.Writer.TryWrite(action);
    public bool Pair(string label, bool editLists, bool register, bool upload, bool readRetainers=false)
    {
        CancelQueuedInventory();
        return Offer(async () => {
        var scopes=new List<string>(Protocol.DefaultScopes);
        if(editLists)scopes.Add("lists:write");
        if(readRetainers||register||upload)scopes.Add("retainers:read");
        if(register)scopes.Add("retainers:register");
        if(upload)scopes.Add("retainers:upload");
        await connection!.BeginPairing(label,scopes.ToArray(),stop.Token);
        });
    }
    public bool CancelPairing()=>Offer(()=>{connection!.CancelPairing();return Task.CompletedTask;});
    public bool Disconnect(){CancelQueuedInventory();return Offer(async()=>{
        await StopFeed();
        await connection!.Disconnect();cart?.Dispose();cart=null;lists=[];characters=[];inbox.Clear();shown.Clear();loadedDevice=null;estimate=null;rosterStatus="";
        ClearRetainerPrices();
        while(toasts.TryDequeue(out _)){}queuedToasts.Clear();CancelQueuedCaptures();
        status="Disconnected. Saved cart work remains bound to its original account and server.";
    });}
    public bool Refresh()=>Offer(async()=>{await RefreshAccount();nextNotifications=DateTimeOffset.MinValue;nextCart=DateTimeOffset.MinValue;});
    public bool SelectList(int id){CancelQueuedInventory();return Offer(()=>OpenList(id));}
    private async Task OpenList(int id)
    {
        CancelQueuedInventory();
        if(!lists.Any(l=>l.List.Id==id))throw new InvalidOperationException("Refresh the available lists first.");
        if(cart?.Document.View.AwaitingPersist==true)cart.Save();
        cart?.Dispose();cart=null;estimate=null;
        cart=new ConnectedCart(connection!,id);await SyncCart();nextCart=DateTimeOffset.UtcNow.AddSeconds(60);
        AtomicFile.Write(SelectionPath(),JsonSerializer.SerializeToUtf8Bytes(id));
    }
    private string SelectionPath()=>Path.Combine(connection!.Directory,"accounts",connection.Session!.AccountId,"selected-list.json");
    public bool SetCurrentWorld(int current,int home)=>Offer(async()=>{
        worlds??=await connection!.Public<WorldDirectory>("/api/v1/world_data",stop.Token);
        priceRegion=worlds.RegionFor(current,home);
    });
    public bool Edit(object edit)=>Offer(()=>{
        if(!CanEdit())throw new InvalidOperationException("Connect with list editing and select a writable cart.");
        cart!.Edit(edit);estimate=null;nextCart=DateTimeOffset.MinValue;status="Saved locally; synchronizing…";return Task.CompletedTask;
    });
    public bool AddItem(ListEditTarget target,int item,string quality="any",int quantity=1)
    {
        if(!target.Matches(View)||item<=0||quantity is <1 or >1_000_000||quality is not ("any" or "nq" or "hq"))return false;
        return Offer(()=>{
            if(!target.Matches(connection?.Session,cart?.Id,CanEdit())) {
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
        if(generation!=InventoryGeneration||!target.Matches(View)||gains.Length>4096)return false;
        var pending=gains.ToArray();
        return Offer(()=>{
            if(generation!=InventoryGeneration||!target.Matches(connection?.Session,cart?.Id,CanEdit()))return Task.CompletedTask;
            var deltas=InventoryProgress.Allocate(cart!.Document.View.Rows,pending);
            long applied=0;
            foreach(var delta in deltas) {
                if(generation!=InventoryGeneration)break;
                cart.Edit(new{kind="add_acquired",item_id=delta.ItemId,quality=delta.Quality,delta=delta.Quantity});
                applied+=delta.Quantity;
            }
            if(applied>0){estimate=null;nextCart=DateTimeOffset.MinValue;status=$"Marked {applied:N0} units acquired from inventory. Saved locally; synchronizing…";}
            return Task.CompletedTask;
        });
    }
    public bool RetrySave()=>Offer(()=>{cart?.Save();nextCart=DateTimeOffset.MinValue;return Task.CompletedTask;});
    public bool UseServerCopy()=>Offer(()=>{cart?.UseServer();nextCart=DateTimeOffset.MinValue;return Task.CompletedTask;});
    public bool RetryRecovery()=>Offer(()=>{cart?.Backup("recovery-original");cart?.Document.RetryRecovery();cart?.Save();nextCart=DateTimeOffset.MinValue;return Task.CompletedTask;});
    public bool Estimate()=>Offer(async()=>{
        if(cart is null)throw new InvalidOperationException("Select a cart first.");
        await SyncCart();
        estimate=await connection!.Send<CartEstimate>(HttpMethod.Get,$"/api/v1/plugin/lists/{cart.Id}/estimate",null,stop.Token);
        status="Market estimate refreshed. Prices are not reserved; whole stacks may cost more.";
    });
    public bool Observe(ObservedCharacter observation)=>Offer(async()=>{
        characters=await connection!.Send<UnverifiedCharacter[]>(HttpMethod.Post,"/api/v1/plugin/characters/observe",observation,stop.Token);
        rosterStatus=status="Character saved as an unverified game observation. Save its retainers to add them to your account.";
    });
    internal static UnverifiedCharacter? LatestObservation(IEnumerable<UnverifiedCharacter> roster,ObservedCharacter observation)=>
        roster.Where(c=>c.Name==observation.Name&&c.HomeWorld==observation.HomeWorld)
            .OrderByDescending(c=>c.ObservedAt).FirstOrDefault();
    public bool Register(ObservedCharacter observation)=>Offer(async()=>{
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
    public bool UploadConsent(string character,bool enabled,bool delete=false)=>Offer(async()=>{
        characters=await connection!.Send<UnverifiedCharacter[]>(HttpMethod.Post,$"/api/v1/plugin/characters/{PositiveId(character)}/uploads",new{enabled,delete_snapshots=delete},stop.Token);
        status=enabled?"Uploads enabled for this character. Revisit a retainer's selling list.":"Uploads paused. Queued captures from the previous consent epoch are invalid.";
    });
    public bool Upload(string retainer,ListingCapture capture)
    {
        if(Interlocked.Increment(ref queuedCaptures)>10){Interlocked.Decrement(ref queuedCaptures);return false;}
        var account=View.Session?.AccountId;
        var generation=Volatile.Read(ref captureGeneration);
        var accepted=Offer(async()=>{try{
        if(generation!=Volatile.Read(ref captureGeneration)||account is null||connection!.Session?.AccountId!=account||DateTimeOffset.UtcNow-capture.ObservedAt>TimeSpan.FromMinutes(10))return;
        // Identical receipt key/body survive a single bounded retry. No disk queue of raw game identifiers.
        try {await connection!.Send<CaptureReceipt>(HttpMethod.Post,$"/api/v1/plugin/retainers/{PositiveId(retainer)}/listings",capture,stop.Token);}
        catch(HttpRequestException) {await connection!.Send<CaptureReceipt>(HttpMethod.Post,$"/api/v1/plugin/retainers/{PositiveId(retainer)}/listings",capture,stop.Token);}
        characters=await connection!.Send<UnverifiedCharacter[]>(HttpMethod.Get,"/api/v1/plugin/characters",null,stop.Token);
        status="Private retainer listing snapshot uploaded.";
        }finally{Interlocked.Decrement(ref queuedCaptures);}});
        if(!accepted)Interlocked.Decrement(ref queuedCaptures);
        return accepted;
    }
    public bool TestNotification()=>Offer(async()=>{
        _=await connection!.Send<bool>(HttpMethod.Post,"/api/v1/plugin/notifications/test",new{operation_id=Guid.NewGuid().ToString("N")},stop.Token);
        nextNotifications=DateTimeOffset.MinValue;
    });
    public bool MarkRead(string id)=>Offer(async()=>{
        _=await connection!.Send<bool>(HttpMethod.Post,"/api/v1/plugin/notifications/read",new{ids=new[]{PositiveId(id)}},stop.Token);
        status="Marked read on Ultros.";
    });
    public bool ToastShown(string id)=>Offer(async()=>{
        shown.Add(id);SaveInbox();
        _=await connection!.Send<bool>(HttpMethod.Post,"/api/v1/plugin/notifications/ack",new{ids=new[]{id}},stop.Token);
    });
    public bool TryToast(out GameDelivery? delivery)=>toasts.TryDequeue(out delivery);
    private void ClearRetainerPrices(){retainerPrices=null;retainerPriceStatus="";pricesDevice=pricesAccount=null;}
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
    private bool CanEdit()=>connection?.Connected==true&&connection.HasScope("lists:write")&&cart is not null
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
        if(connection.HasScope("retainers:read"))characters=await connection.Send<UnverifiedCharacter[]>(HttpMethod.Get,"/api/v1/plugin/characters",null,stop.Token);
        nextLists=DateTimeOffset.UtcNow.AddSeconds(60);
    }
    private void LoadInbox()
    {
        inbox.Clear();shown.Clear();queuedToasts.Clear();
        while(toasts.TryDequeue(out _)){}
        var session=connection!.Session!;
        loadedDevice=session.Id;
        var dir=Path.Combine(connection.Directory,"accounts",session.AccountId,"notifications");
        Directory.CreateDirectory(dir);inboxPath=Path.Combine(dir,session.Id+".json");
        if(File.Exists(inboxPath)&&new FileInfo(inboxPath).Length<=512*1024)
        {
            try {
                var saved=JsonSerializer.Deserialize<SavedInbox>(File.ReadAllBytes(inboxPath),Protocol.Json);
                if(saved is not null) {inbox.AddRange(saved.Entries.Take(100));shown.UnionWith(saved.Shown.Take(1000));}
            }catch(JsonException){status="Local notification history could not be read.";}
        }
        foreach(var item in inbox.Where(d=>d.ExpiresAt>DateTimeOffset.UtcNow&&!shown.Contains(d.Id))){toasts.Enqueue(item);queuedToasts.Add(item.Id);}
    }
    private void SaveInbox()
    {
        if(inboxPath is null)return;
        while(inbox.Count>100)inbox.RemoveAt(0);
        shown.IntersectWith(inbox.Select(i=>i.Id));
        AtomicFile.Write(inboxPath,JsonSerializer.SerializeToUtf8Bytes(new SavedInbox(inbox.ToArray(),shown.ToArray()),Protocol.Json));
    }
    private async Task PollNotifications()
    {
        var batch=await connection!.Send<DeliveryBatch>(HttpMethod.Get,"/api/v1/plugin/notifications",null,stop.Token);
        foreach(var item in batch.Deliveries)
        {
            if(item.ExpiresAt<=DateTimeOffset.UtcNow)continue;
            if(!inbox.Any(i=>i.Id==item.Id))inbox.Add(item);
        }
        SaveInbox();
        foreach(var item in batch.Deliveries)
        {
            if(item.ExpiresAt<=DateTimeOffset.UtcNow||shown.Contains(item.Id)||!queuedToasts.Add(item.Id))continue;
            toasts.Enqueue(item);
        }
        var retry=batch.Deliveries.Where(i=>shown.Contains(i.Id)).Select(i=>i.Id).ToArray();
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
        var showPrices=connected&&connection!.HasScope("retainers:read")
            &&pricesDevice==connection.Session?.Id&&pricesAccount==connection.Session?.AccountId;
        Volatile.Write(ref view,new ClientView(connected,status,connection?.StorageMode??"",connection?.HumanCode,connection?.VerificationUrl??endpoint.At("/plugin/verify").AbsoluteUri,
            connection?.Session,connected?lists:[],cart?.Id,cart?.Document.View,CanEdit(),connected?characters:[],connected?inbox.ToArray():[],market,sales,estimate,connection?.Session?.Scopes??[],priceRegion,Volatile.Read(ref feedReady)!=0?"Live notifications (SSE)":"Polling fallback",connected?rosterStatus:"",registering,connection?.ApprovalUrl,
            showPrices?retainerPrices:null,showPrices?retainerPriceStatus:""));
    }
    private async Task Run()
    {
        try
        {
            connection=new Connection(root,endpoint,profile);
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
                    if(await connection.PollPairing(stop.Token)){status=connection.Status;nextLists=nextNotifications=nextCart=DateTimeOffset.MinValue;}
                    if(connection.Connected)
                    {
                        if(loadedDevice!=connection.Session!.Id){CancelQueuedInventory();await StopFeed();cart?.Dispose();cart=null;estimate=null;rosterStatus="";ClearRetainerPrices();LoadInbox();nextLists=nextNotifications=DateTimeOffset.MinValue;}
                        if(DateTimeOffset.UtcNow>=nextLists)await RefreshAccount();
                        if(connection.HasScope("notifications:read"))await UpdateFeed();
                        if(DateTimeOffset.UtcNow>=nextNotifications&&connection.HasScope("notifications:read"))await PollNotifications();
                        if(DateTimeOffset.UtcNow>=nextCart&&cart is not null){await SyncCart();nextCart=DateTimeOffset.UtcNow.AddSeconds(60);}
                    }
                    else if(notificationFeed is not null)await StopFeed();
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
