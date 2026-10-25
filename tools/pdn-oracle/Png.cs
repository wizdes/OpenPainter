using System;
using System.Buffers.Binary;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Text;

internal sealed class Png
{
    internal readonly int Width, Height;
    internal readonly byte[] Pixels;
    internal Png(int width, int height) { Width=width; Height=height; Pixels=new byte[checked(width*height*4)]; }
    internal void Set(int x,int y,byte r,byte g,byte b,byte a) { int p=(y*Width+x)*4; Pixels[p]=r; Pixels[p+1]=g; Pixels[p+2]=b; Pixels[p+3]=a; }
    internal static Png Read(string path)
    {
        using var f=File.OpenRead(path); using var reader=new BinaryReader(f);
        byte[] sig=reader.ReadBytes(8); if (!sig.AsSpan().SequenceEqual(new byte[]{137,80,78,71,13,10,26,10})) throw new InvalidDataException("PNG signature");
        int w=0,h=0; using var idat=new MemoryStream();
        while(f.Position<f.Length) {
            int length=checked((int)Read32(reader)); string type=Encoding.ASCII.GetString(reader.ReadBytes(4)); byte[] data=reader.ReadBytes(length); uint crc=Read32(reader);
            byte[] kind=Encoding.ASCII.GetBytes(type); if(Crc(kind,data)!=crc) throw new InvalidDataException("PNG CRC");
            if(type=="IHDR") { w=checked((int)BinaryPrimitives.ReadUInt32BigEndian(data));h=checked((int)BinaryPrimitives.ReadUInt32BigEndian(data.AsSpan(4)));if(data[8]!=8||data[9]!=6||data[12]!=0)throw new InvalidDataException("Only noninterlaced RGBA8 PNG is supported"); }
            if(type=="IDAT")idat.Write(data); if(type=="IEND")break;
        }
        if(w<1||h<1)throw new InvalidDataException("PNG size"); var image=new Png(w,h); idat.Position=0;
        using var z=new ZLibStream(idat,CompressionMode.Decompress); int stride=w*4; byte[] row=new byte[stride+1],prev=new byte[stride];
        for(int y=0;y<h;y++) { z.ReadExactly(row); int filter=row[0]; for(int x=0;x<stride;x++) { int left=x>=4?row[x-3]:0,up=prev[x],ul=x>=4?prev[x-4]:0; int predictor=filter switch {0=>0,1=>left,2=>up,3=>(left+up)/2,4=>Paeth(left,up,ul),_=>throw new InvalidDataException("PNG filter")};row[x+1]=unchecked((byte)(row[x+1]+predictor)); } Buffer.BlockCopy(row,1,image.Pixels,y*stride,stride);Buffer.BlockCopy(row,1,prev,0,stride); }
        return image;
    }
    internal void Write(string path)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);using var f=File.Create(path);using var writer=new BinaryWriter(f);
        writer.Write(new byte[]{137,80,78,71,13,10,26,10});byte[] header=new byte[13];BinaryPrimitives.WriteUInt32BigEndian(header,(uint)Width);BinaryPrimitives.WriteUInt32BigEndian(header.AsSpan(4),(uint)Height);header[8]=8;header[9]=6;Chunk(writer,"IHDR",header);
        bool subFilter=new[]{"Fragment_Blur_","Median_Blur_","Motion_Blur_","Radial_Blur_","Zoom_Blur_","Reduce_Noise_","Glow_","Red_Eye_Removal_","Sharpen_","Soften_Portrait_"}.Any(prefix=>Path.GetFileName(path).StartsWith(prefix,StringComparison.Ordinal));
        using var raw=new MemoryStream();using(var z=new ZLibStream(raw,CompressionLevel.SmallestSize,true)) { for(int y=0;y<Height;y++){if(subFilter){z.WriteByte(1);byte[] row=new byte[Width*4];for(int i=0;i<row.Length;i++)row[i]=unchecked((byte)(Pixels[y*Width*4+i]-(i>=4?Pixels[y*Width*4+i-4]:0)));z.Write(row);}else{z.WriteByte(0);z.Write(Pixels,y*Width*4,Width*4);}} }Chunk(writer,"IDAT",raw.ToArray());Chunk(writer,"IEND",Array.Empty<byte>());
    }
    private static int Paeth(int a,int b,int c){int p=a+b-c,da=Math.Abs(p-a),db=Math.Abs(p-b),dc=Math.Abs(p-c);return da<=db&&da<=dc?a:db<=dc?b:c;}
    private static uint Read32(BinaryReader r){byte[] b=r.ReadBytes(4);if(b.Length!=4)throw new EndOfStreamException();return BinaryPrimitives.ReadUInt32BigEndian(b);}
    private static void Chunk(BinaryWriter w,string type,byte[] bytes){byte[] kind=Encoding.ASCII.GetBytes(type),len=new byte[4],crc=new byte[4];BinaryPrimitives.WriteUInt32BigEndian(len,(uint)bytes.Length);BinaryPrimitives.WriteUInt32BigEndian(crc,Crc(kind,bytes));w.Write(len);w.Write(kind);w.Write(bytes);w.Write(crc);}
    private static uint Crc(byte[] kind,byte[] data){uint c=0xffffffff;foreach(byte b in kind)c=Step(c,b);foreach(byte b in data)c=Step(c,b);return ~c;}
    private static uint Step(uint c,byte b){c^=b;for(int i=0;i<8;i++)c=(c&1)!=0?0xedb88320^(c>>1):c>>1;return c;}
}
