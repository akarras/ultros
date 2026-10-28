using System.Net.WebSockets;
using System.Text;
using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class ListFeedChecks
{
    private sealed class Frames(IEnumerable<(byte[] Data,bool End,WebSocketMessageType Type)> frames):WebSocket
    {
        private readonly Queue<(byte[] Data,bool End,WebSocketMessageType Type)> pending=new(frames);
        public override WebSocketCloseStatus? CloseStatus=>null;
        public override string? CloseStatusDescription=>null;
        public override WebSocketState State=>WebSocketState.Open;
        public override string? SubProtocol=>null;
        public override void Abort(){}
        public override void Dispose(){}
        public override Task CloseAsync(WebSocketCloseStatus status,string? reason,CancellationToken token)=>Task.CompletedTask;
        public override Task CloseOutputAsync(WebSocketCloseStatus status,string? reason,CancellationToken token)=>Task.CompletedTask;
        public override Task SendAsync(ArraySegment<byte> buffer,WebSocketMessageType type,bool end,CancellationToken token)=>throw new NotSupportedException();
        public override Task<WebSocketReceiveResult> ReceiveAsync(ArraySegment<byte> buffer,CancellationToken token)
        {
            token.ThrowIfCancellationRequested();
            if(!pending.TryDequeue(out var frame))return Task.FromResult(new WebSocketReceiveResult(0,WebSocketMessageType.Close,true));
            var count=Math.Min(buffer.Count,frame.Data.Length);frame.Data.AsSpan(0,count).CopyTo(buffer.AsSpan());
            return Task.FromResult(new WebSocketReceiveResult(count,frame.Type,frame.End));
        }
    }
    internal static async Task Run()
    {
        var body=Encoding.UTF8.GetBytes("{\"event\":\"changed\",\"list_id\":17}");
        using(var socket=new Frames(body.Chunk(3).Select((part,index)=>(part,index==(body.Length-1)/3,WebSocketMessageType.Text)))) {
            var wakes=0;await ListFeed.Read(socket,17,()=>wakes++,CancellationToken.None);
            if(wakes!=1)throw new Exception("Fragmented list wake did not trigger exactly one sync.");
        }
        using(var truncated=new Frames([(body[..10],false,WebSocketMessageType.Text)])) {
            var wakes=0;await ListFeed.Read(truncated,17,()=>wakes++,CancellationToken.None);
            if(wakes!=0)throw new Exception("Truncated wake triggered a sync.");
        }
        foreach(var payload in new[]{"{\"event\":\"changed\",\"list_id\":18}","{\"event\":\"unrelated\",\"list_id\":17}",new string('x',513)}) {
            using var socket=new Frames(Encoding.UTF8.GetBytes(payload).Chunk(128).Select(part=>(part,false,WebSocketMessageType.Text)).Append((Array.Empty<byte>(),true,WebSocketMessageType.Text)));
            var rejected=false;
            try{await ListFeed.Read(socket,17,()=>throw new Exception("Invalid event triggered a sync."),CancellationToken.None);}
            catch(Exception e)when(e is IOException or System.Text.Json.JsonException){rejected=true;}
            if(!rejected)throw new Exception("Unbounded or unrelated list event was accepted.");
        }
        using(var binary=new Frames([(body,true,WebSocketMessageType.Binary)])) {
            try{await ListFeed.Read(binary,17,()=>{},CancellationToken.None);throw new Exception("Binary event was accepted.");}
            catch(IOException){}
        }
        Console.WriteLine("PASS: bounded, fragmented, truncated and list-scoped WebSocket wake-ups.");
    }
}
