using System.Net.WebSockets;
using System.Text.Json;
namespace Ultros.Client;

internal sealed class ListFeed : IDisposable
{
    private readonly CancellationTokenSource lifetime;
    internal Task Completion { get; }
    internal ListFeed(Transport transport,string access,int list,Action wake,CancellationToken stop)
    {
        lifetime=CancellationTokenSource.CreateLinkedTokenSource(stop);
        lifetime.CancelAfter(TimeSpan.FromMinutes(11));
        Completion=transport.ListenList(access,list,wake,lifetime.Token);
    }
    private bool disposed;
    public void Dispose(){if(disposed)return;disposed=true;lifetime.Cancel();lifetime.Dispose();}

    internal static async Task Read(WebSocket socket,int list,Action wake,CancellationToken cancellation)
    {
        var buffer=new byte[512];var used=0;
        while(true)
        {
            if(used==buffer.Length)throw new IOException("List event exceeds the client limit.");
            var message=await socket.ReceiveAsync(buffer.AsMemory(used),cancellation).ConfigureAwait(false);
            if(message.MessageType==WebSocketMessageType.Close)return;
            if(message.MessageType!=WebSocketMessageType.Text)throw new IOException("Unexpected list event type.");
            used+=message.Count;
            if(!message.EndOfMessage)continue;
            using var json=JsonDocument.Parse(buffer.AsMemory(0,used));
            var frame=json.RootElement;
            if(frame.ValueKind!=JsonValueKind.Object
                ||!frame.TryGetProperty("event",out var kind)||kind.GetString()!="changed"
                ||!frame.TryGetProperty("list_id",out var id)||!id.TryGetInt32(out var actual)||actual!=list)
                throw new IOException("Unexpected list event.");
            wake();used=0;
        }
    }
}
