using System.Net;
using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class TransportDeadlineChecks
{
    internal static async Task Run()
    {
        using var handler=new StallingHandler();
        using var transport=new Transport(new UltrosEndpoint(false),handler,TimeSpan.FromMilliseconds(500));
        using var watchdog=new CancellationTokenSource(TimeSpan.FromSeconds(5));
        try {
            _=await transport.Send<bool>(HttpMethod.Get,"/fixture",null,null,watchdog.Token);
            throw new Exception("A stalled response body completed successfully.");
        }
        catch(OperationCanceledException) when(!watchdog.IsCancellationRequested){}
        if(watchdog.IsCancellationRequested)throw new Exception("Response body relied on caller cancellation instead of its request deadline.");
        if(!handler.Stream.ReadStarted)throw new Exception("Fixture did not stall after response headers.");
    }
    private sealed class StallingHandler:HttpMessageHandler
    {
        internal readonly StallingStream Stream=new();
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request,CancellationToken cancellationToken)=>
            Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK){Content=new StreamContent(Stream)});
    }
    private sealed class StallingStream:Stream
    {
        internal bool ReadStarted;
        public override bool CanRead=>true;
        public override bool CanSeek=>false;
        public override bool CanWrite=>false;
        public override long Length=>throw new NotSupportedException();
        public override long Position{get=>throw new NotSupportedException();set=>throw new NotSupportedException();}
        public override async ValueTask<int> ReadAsync(Memory<byte> buffer,CancellationToken cancellationToken=default)
        {
            ReadStarted=true;
            await Task.Delay(Timeout.Infinite,cancellationToken);
            return 0;
        }
        public override int Read(byte[] buffer,int offset,int count)=>throw new NotSupportedException();
        public override void Flush()=>throw new NotSupportedException();
        public override long Seek(long offset,SeekOrigin origin)=>throw new NotSupportedException();
        public override void SetLength(long value)=>throw new NotSupportedException();
        public override void Write(byte[] buffer,int offset,int count)=>throw new NotSupportedException();
    }
}
