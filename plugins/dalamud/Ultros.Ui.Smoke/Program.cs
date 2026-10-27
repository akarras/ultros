using System.Drawing.Imaging;
using System.Numerics;
using System.Runtime.InteropServices;
using Dalamud.Bindings.ImGui;
using Ultros.Client;
using UltrosPlugin.Ui;

// Render the shipped native components using Dalamud's actual ImGui binding.
// No game process, credentials or additional rendering libraries are required.
var output=args.FirstOrDefault()??Path.Combine("artifacts","dalamud","ui");
Directory.CreateDirectory(output);
var now=DateTimeOffset.UtcNow;
string[] scopes=["lists:read","lists:write","retainers:read","retainers:register","retainers:upload","notifications:read","notifications:ack","notifications:mark-read"];
var session=new DeviceSession("preview-device","My game client","123456789012345678","chewbert",scopes,now,now,now.AddDays(1),1);
var deliveries=new[]{
    new GameDelivery("1",null,"test","Ultros test notification","This is your connected game client.",new("none",null,null),now.AddMinutes(5)),
    new GameDelivery("2",24,"alert","Price alert: Grade 8 Dark Matter","The price on Gilgamesh is now 280 gil. Your target is 300 gil.",new("open_item",10386,null),now.AddHours(1))};
var view=new ClientView(true,"Connected.","Credentials protected by Windows.",null,"https://ultros.app/plugin/verify",session,[],null,null,true,[],deliveries,null,[],null,scopes,NotificationTransport:"Live alerts",ReadNotificationIds:["1"]);
var retainerView=view with{Characters=[new("1","Mira Example",63,true,false,true,1,now,[new("1","Copper Finch",1,true,1,now,false,1)])],
    RetainerSnapshots=[new("1",1,now,now,[new(0,8,false,99,45),new(1,5056,true,1,188330)])]};
foreach(var width in new[]{940,650})
{
    Render("account",width,Page.Account,view);
    Render("history",width,Page.Notifications,view);
    Render("retainers",width,Page.Retainers,retainerView);
}
Render("connect",940,Page.Account,view with{Connected=false,Status="Disconnected.",Session=null,Inbox=[]});
Render("approval",650,Page.Account,view with{Connected=false,Status="Waiting for browser approval.",Session=null,Inbox=[],HumanCode="ABCD-EFGH",ApprovalUrl="https://ultros.app/plugin/verify#pair=fixture"});
Render("empty-history",650,Page.Notifications,view with{Inbox=[]});
Render("account-large-font",650,Page.Account,view,22);
Render("retainers-large-font",650,Page.Retainers,retainerView,22);
Console.WriteLine($"Native ImGui previews rendered to {Path.GetFullPath(output)}");

unsafe void Render(string name,int width,Page selected,ClientView state,float fontSize=17)
{
    ImGui.CreateContext();
    try
    {
        ImGui.StyleColorsDark();
        var io=ImGui.GetIO();io.DisplaySize=new(width,700);io.DeltaTime=1f/60;io.IniFilename=null;
        io.Fonts.AddFontFromFileTTF(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Fonts),"segoeui.ttf"),fontSize,default,null);
        byte* atlas=null;int aw=0,ah=0;io.Fonts.GetTexDataAsRGBA32(0,&atlas,&aw,&ah);
        io.Fonts.SetTexID(0,new ImTextureID(1));
        var pixels=new byte[aw*ah*4];Marshal.Copy((IntPtr)atlas,pixels,0,pixels.Length);
        var account=new AccountPanel();var history=new NotificationPanel();
        var retainers=new RetainerListingsPanel{ItemName=id=>id==8?"Ice Crystal":"Courtly Lover's Shoes of Healing",WorldName=_=>"Gilgamesh"};
        var log=Path.GetFullPath(Path.Combine(output,$"{name}-{width}.txt"));File.WriteAllText(log,"");
        // Let ImGui settle the child positions before capturing draw data.
        for(var frame=0;frame<3;frame++)
        {
            ImGui.NewFrame();ImGui.SetNextWindowPos(Vector2.Zero);ImGui.SetNextWindowSize(new(width,700));ImGui.SetNextWindowBgAlpha(1);
            if(frame==2)ImGui.LogToFile(0,log);
            UiKit.BeginStyle();
            ImGui.Begin("Ultros · native preview",ImGuiWindowFlags.NoResize|ImGuiWindowFlags.NoMove|ImGuiWindowFlags.NoCollapse);
            selected=UiKit.Navigation(selected,state.Connected,state.Inbox.Length);
            UiKit.BeginContent();
            account.Track(state,selected==Page.Account);
            if(selected==Page.Account)account.Draw(state);
            else if(selected==Page.Retainers){UiKit.Heading("Retainers","Your uploaded listings, grouped by character and retainer.");retainers.Draw(state);}
            else history.Draw(state);
            UiKit.EndContent(state.Status);ImGui.End();UiKit.EndStyle();if(frame==2)ImGui.LogFinish();ImGui.Render();
        }
        var drawn=File.ReadAllText(log);
        foreach(var secret in new[]{session.AccountName,session.AccountId,"ABCD-EFGH"})
            if(drawn.Contains(secret,StringComparison.Ordinal))throw new Exception($"Private identity or pairing code visible in {name}.");
        if(state.Connected&&selected==Page.Account&&drawn.Contains("Connect to Ultros",StringComparison.Ordinal))
            throw new Exception("A connected account still shows setup instructions.");
        WritePng(ImGui.GetDrawData(),pixels,aw,ah,width,700,Path.Combine(output,$"{name}-{width}.png"));
    }
    finally{ImGui.DestroyContext();}
}

static unsafe void WritePng(ImDrawDataPtr data,byte[] atlas,int aw,int ah,int width,int height,string path)
{
    var rgba=new byte[width*height*4];
    for(var i=0;i<rgba.Length;i+=4){rgba[i]=18;rgba[i+1]=18;rgba[i+2]=20;rgba[i+3]=255;}
    for(var n=0;n<data.CmdListsCount;n++)
    {
        var list=new ImDrawListPtr(data.CmdLists[n]);
        foreach(var command in list.CmdBuffer)
        {
            if(command.UserCallback!=null)continue;
            var clip=command.ClipRect;
            for(var index=0u;index<command.ElemCount;index+=3)
            {
                var a=list.VtxBuffer[(int)(list.IdxBuffer[(int)(command.IdxOffset+index)]+command.VtxOffset)];
                var b=list.VtxBuffer[(int)(list.IdxBuffer[(int)(command.IdxOffset+index+1)]+command.VtxOffset)];
                var c=list.VtxBuffer[(int)(list.IdxBuffer[(int)(command.IdxOffset+index+2)]+command.VtxOffset)];
                var area=Cross(b.Pos-a.Pos,c.Pos-a.Pos);if(Math.Abs(area)<.001f)continue;
                var minX=(int)Math.Max(0,Math.Max(clip.X,Math.Floor(Math.Min(a.Pos.X,Math.Min(b.Pos.X,c.Pos.X)))));
                var maxX=(int)Math.Min(width,Math.Min(clip.Z,Math.Ceiling(Math.Max(a.Pos.X,Math.Max(b.Pos.X,c.Pos.X)))));
                var minY=(int)Math.Max(0,Math.Max(clip.Y,Math.Floor(Math.Min(a.Pos.Y,Math.Min(b.Pos.Y,c.Pos.Y)))));
                var maxY=(int)Math.Min(height,Math.Min(clip.W,Math.Ceiling(Math.Max(a.Pos.Y,Math.Max(b.Pos.Y,c.Pos.Y)))));
                for(var y=minY;y<maxY;y++)for(var x=minX;x<maxX;x++)
                {
                    var p=new Vector2(x+.5f,y+.5f);
                    var u=Cross(b.Pos-p,c.Pos-p)/area;var v=Cross(c.Pos-p,a.Pos-p)/area;var w=1-u-v;
                    if(u<0||v<0||w<0)continue;
                    var uv=u*a.Uv+v*b.Uv+w*c.Uv;
                    var tx=Math.Clamp((int)(uv.X*aw),0,aw-1);var ty=Math.Clamp((int)(uv.Y*ah),0,ah-1);
                    var tex=(ty*aw+tx)*4;var dst=(y*width+x)*4;
                    float Color(int channel)=>u*((a.Col>>(channel*8))&255)+v*((b.Col>>(channel*8))&255)+w*((c.Col>>(channel*8))&255);
                    var alpha=Color(3)/255*atlas[tex+3]/255;
                    for(var channel=0;channel<3;channel++)rgba[dst+channel]=(byte)(Color(channel)*atlas[tex+channel]/255*alpha+rgba[dst+channel]*(1-alpha));
                }
            }
        }
    }
    using var bitmap=new Bitmap(width,height,PixelFormat.Format32bppArgb);
    var locked=bitmap.LockBits(new Rectangle(0,0,width,height),ImageLockMode.WriteOnly,PixelFormat.Format32bppArgb);
    for(var i=0;i<rgba.Length;i+=4)(rgba[i],rgba[i+2])=(rgba[i+2],rgba[i]);
    try{Marshal.Copy(rgba,0,locked.Scan0,rgba.Length);}finally{bitmap.UnlockBits(locked);}
    bitmap.Save(path,ImageFormat.Png);
}
static float Cross(Vector2 a,Vector2 b)=>a.X*b.Y-a.Y*b.X;
