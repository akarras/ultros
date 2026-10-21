using System.Text;
namespace Ultros.Client;

internal sealed class NotificationFeed : IDisposable
{
    private readonly CancellationTokenSource lifetime;
    internal Task Completion { get; }
    internal NotificationFeed(Transport transport,string access,Action wake,CancellationToken stop)
    {
        lifetime=CancellationTokenSource.CreateLinkedTokenSource(stop);
        lifetime.CancelAfter(TimeSpan.FromMinutes(3));
        Completion=transport.ListenNotifications(access,wake,lifetime.Token);
    }
    private bool disposed;
    public void Dispose(){if(disposed)return;disposed=true;lifetime.Cancel();lifetime.Dispose();}

    // Bound both lines and complete frames; fragmented reads/CRLF/comments are
    // valid, but a truncated frame must not trigger a delivery fetch.
    internal static async Task Read(Stream stream,Action wake,CancellationToken cancellation)
    {
        var buffer=new byte[2048];
        var line=new StringBuilder();
        var eventName="";var frameBytes=0;
        while(true)
        {
            var count=await stream.ReadAsync(buffer,cancellation).ConfigureAwait(false);
            if(count==0)return;
            for(var i=0;i<count;i++)
            {
                var value=buffer[i];
                if(++frameBytes>8192)throw new IOException("Notification event exceeds the client limit.");
                if(value!=(byte)'\n')
                {
                    if(line.Length>=1024)throw new IOException("Notification line exceeds the client limit.");
                    line.Append((char)value);continue;
                }
                var text=line.ToString().TrimEnd('\r');line.Clear();
                if(text.Length==0)
                {
                    if(eventName=="notifications")wake();
                    eventName="";frameBytes=0;
                }
                else if(text.StartsWith("event:",StringComparison.Ordinal))eventName=text[6..].Trim();
            }
        }
    }
}
