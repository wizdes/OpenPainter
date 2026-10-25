using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text.Json;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

internal static class Program
{
    private const string Commit="cca476b0df2a2f70996e6b9486ec45327631568c";
    private static string Root=>Directory.GetCurrentDirectory();
    private static string Fixtures=>Path.Combine(Root,"tests/fixtures");
    private static string Oracle=>Path.Combine(Root,"tests/oracle/legacy336");
    private static readonly JsonSerializerOptions JsonOptions=new(){WriteIndented=true};
    private record Entry(string commit,string dotnet,string op,object parameters,string input,string input_sha256,string output,string output_sha256,string status,string notes);
    private static List<Entry> entries=new();
    private static string Hash(string path)=>Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(path))).ToLowerInvariant();
    private static void Record(string dir,string op,object parameters,string input,string output,string status="ok",string notes="")
    {
        string inputName=input==""?"":Path.GetRelativePath(Root,input);
        string outputName=output==""?"":Path.GetRelativePath(Root,output);
        entries.RemoveAll(e=>e.op==op && e.input==inputName && e.output==outputName);
        entries.Add(new(Commit,Environment.Version.ToString(),op,parameters,inputName,input==""?"":Hash(input),outputName,output==""?"":Hash(output),status,notes));
    }
    private static void SaveManifest(string dir){Directory.CreateDirectory(dir);File.WriteAllText(Path.Combine(dir,"manifest.json"),JsonSerializer.Serialize(entries,JsonOptions)+"\n");entries.Clear();}
    private static void Write(string dir,string name,Png png,string op,object parameters,string input="",string notes="",string status="ok") {string path=Path.Combine(dir,name);png.Write(path);Record(dir,op,parameters,input,path,status,notes);}
    private static int Main(string[] args)
    {
        try { if(args.Length==0)throw new ArgumentException("Use fixtures, vectors, blend, resample, flood, effect <name> <json-params>, or effects-all");
            switch(args[0]) {case "fixtures":MakeFixtures();break;case "vectors":Vectors();break;case "blend":Blend();break;case "resample":Resample();break;case "flood":Flood();break;case "effect":if(args.Length<3)throw new ArgumentException("effect needs name and JSON parameters");Effects(args[1],args[2]);break;case "effects-all":EffectsAll();break;default:throw new ArgumentException("Unknown command: "+args[0]);}return 0;
        }catch(Exception e){Console.Error.WriteLine(e);return 1;}
    }
    private static uint Noise(int x,int y,int channel){uint h=(uint)(x*374761393+y*668265263+channel*1442695041+0x51ed270b);h=(h^(h>>13))*1274126177;return h^(h>>16);}
    private static int Smooth(int x,int y,int channel){int gx=x/64,gy=y/64,fx=x%64,fy=y%64;int a=(int)(Noise(gx,gy,channel)&255),b=(int)(Noise(gx+1,gy,channel)&255),c=(int)(Noise(gx,gy+1,channel)&255),d=(int)(Noise(gx+1,gy+1,channel)&255);return (a*(64-fx)*(64-fy)+b*fx*(64-fy)+c*(64-fx)*fy+d*fx*fy+2048)/4096;}
    private static void MakeFixtures(){Directory.CreateDirectory(Fixtures);Png p;
        p=new(64,64);for(int y=0;y<64;y++)for(int x=0;x<64;x++)p.Set(x,y,(byte)(x==32&&y==32?255:0),(byte)(x==32&&y==32?255:0),(byte)(x==32&&y==32?255:0),255);Write(Fixtures,"impulse_64.png",p,"fixture",new{});
        p=new(256,16);for(int y=0;y<16;y++)for(int x=0;x<256;x++)p.Set(x,y,(byte)x,(byte)x,(byte)x,255);Write(Fixtures,"ramp_256x16.png",p,"fixture",new{});
        p=new(64,64);for(int y=0;y<64;y++)for(int x=0;x<64;x++){int covered=0;for(int sy=0;sy<4;sy++)for(int sx=0;sx<4;sx++){int dx=8*x+2*sx+1-256,dy=8*y+2*sy+1-256;if(dx*dx+dy*dy<204*204)covered++;}p.Set(x,y,255,26,13,(byte)((covered*255+8)/16));}Write(Fixtures,"alpha_edge_64.png",p,"fixture",new{});
        p=new(64,64);for(int y=0;y<64;y++)for(int x=0;x<64;x++)p.Set(x,y,83,171,219,77);Write(Fixtures,"partial_alpha_64.png",p,"fixture",new{});
        p=new(600,300);for(int y=0;y<300;y++)for(int x=0;x<600;x++){bool dark=((x/17+y/17)&1)==0;bool seam=x==255||x==256||x==511||x==512||y==255||y==256;p.Set(x,y,seam?(byte)255:dark?(byte)32:(byte)224,seam?(byte)0:dark?(byte)57:(byte)215,seam?(byte)140:dark?(byte)81:(byte)187,255);}Write(Fixtures,"checker_600x300.png",p,"fixture",new{});
        foreach(bool alpha in new[]{false,true}){p=new(512,384);for(int y=0;y<384;y++)for(int x=0;x<512;x++){int n=Smooth(x,y,1);p.Set(x,y,(byte)n,(byte)((3*n+128)/4),(byte)(255-n/2),alpha?(byte)((x*255+255)/511): (byte)255);}Write(Fixtures,alpha?"plasma_alpha_512x384.png":"plasma_512x384.png",p,"fixture",new{seed="0x51ed270b",alpha});}SaveManifest(Fixtures);
    }
    private static string F(string name)=>Path.Combine(Fixtures,name+".png");
    private static ColorBgra Get(Png p,int x,int y){int i=(y*p.Width+x)*4;return ColorBgra.FromRgba(p.Pixels[i],p.Pixels[i+1],p.Pixels[i+2],p.Pixels[i+3]);}
    private static void Put(Png p,int x,int y,ColorBgra c)=>p.Set(x,y,c.R,c.G,c.B,c.A);
    private static Surface SurfaceOf(Png p){var s=new Surface(p.Width,p.Height);for(int y=0;y<p.Height;y++)for(int x=0;x<p.Width;x++)s[x,y]=Get(p,x,y);return s;}
    private static Png PngOf(Surface s){var p=new Png(s.Width,s.Height);for(int y=0;y<s.Height;y++)for(int x=0;x<s.Width;x++)Put(p,x,y,s[x,y]);return p;}
    private static void Vectors()
    {
        string dir=Path.Combine(Oracle,"vectors");Directory.CreateDirectory(dir);
        int[] values=new int[65536];
        for(int a=0;a<256;a++)for(int b=0;b<256;b++)values[(a<<8)|b]=Utility.FastScaleByteByByte((byte)a,(byte)b);
        string path=Path.Combine(dir,"fast_scale_byte_by_byte.json");
        File.WriteAllText(path,JsonSerializer.Serialize(new{commit=Commit,op="Utility.FastScaleByteByByte",index="a*256+b",values})+"\n");
        Record(dir,"Utility.FastScaleByteByByte",new{pairs=65536},"",path);
        int[] weights={0,64,128,192,255};
        int[][] lerp=new int[5][],blend=new int[5][];
        for(int i=0;i<5;i++){lerp[i]=new int[65536];blend[i]=new int[65536];}
        for(int a=0;a<256;a++)for(int b=0;b<256;b++)
        {
            var ca=ColorBgra.FromBgra((byte)a,(byte)a,(byte)a,255);
            var cb=ColorBgra.FromBgra((byte)b,(byte)b,(byte)b,255);
            int index=(a<<8)|b;
            for(int i=0;i<5;i++)
            {
                lerp[i][index]=ColorBgra.Lerp(ca,cb,(double)weights[i]/255).R;
                blend[i][index]=ColorBgra.Blend(ca,cb,(byte)weights[i]).R;
            }
        }
        path=Path.Combine(dir,"color_bgra_lerp.json");
        File.WriteAllText(path,JsonSerializer.Serialize(new{commit=Commit,op="ColorBgra.Lerp",index="a*256+b",fractions=weights.Select(w=>(double)w/255),values=lerp})+"\n");
        Record(dir,"ColorBgra.Lerp",new{pairs=65536,fractions=weights},"",path);
        path=Path.Combine(dir,"color_bgra_blend.json");
        File.WriteAllText(path,JsonSerializer.Serialize(new{commit=Commit,op="ColorBgra.Blend",index="a*256+b",alpha=weights,values=blend})+"\n");
        Record(dir,"ColorBgra.Blend",new{pairs=65536,alpha=weights},"",path);
        path=Path.Combine(dir,"gaussian_row.json");
        File.WriteAllText(path,JsonSerializer.Serialize(new{commit=Commit,op="GaussianBlurEffect.CreateGaussianBlurRow",radius=2,weights=GaussianBlurEffect.CreateGaussianBlurRow(2)})+"\n");
        Record(dir,"GaussianBlurEffect.CreateGaussianBlurRow",new{radius=2},"",path);
        SaveManifest(dir);
    }
    private static void Blend(){string dir=Path.Combine(Oracle,"blend");Directory.CreateDirectory(dir);foreach(var pair in new[]{("plasma_512x384","plasma_alpha_512x384"),("checker_600x300","alpha_edge_64")}){Png lower=Png.Read(F(pair.Item1)),upper=Png.Read(F(pair.Item2));int w=Math.Min(lower.Width,upper.Width),h=Math.Min(lower.Height,upper.Height);foreach(Type t in UserBlendOps.GetBlendOps().OrderBy(t=>t.Name,StringComparer.Ordinal)){string name=t.Name.Replace("BlendOp","");if(!new[]{"Normal","Multiply","Additive","ColorBurn","ColorDodge","Reflect","Glow","Overlay","Difference","Negation","Lighten","Darken","Screen","Xor"}.Contains(name))continue;foreach(int opacity in new[]{0,64,128,192,255}){var op=UserBlendOps.CreateBlendOp(t).CreateWithOpacity(opacity);var result=new Png(w,h);for(int y=0;y<h;y++)for(int x=0;x<w;x++)Put(result,x,y,op.Apply(Get(lower,x,y),Get(upper,x,y)));Write(dir,$"{pair.Item1}_{pair.Item2}_{name}_{opacity}.png",result,name,new{opacity,upper=pair.Item2},F(pair.Item1),"Upper input SHA-256: "+Hash(F(pair.Item2)));}}}SaveManifest(dir);}
    private static void Resample(){string dir=Path.Combine(Oracle,"resample");Directory.CreateDirectory(dir);string input=F("plasma_alpha_512x384");using var source=SurfaceOf(Png.Read(input));foreach((string label,int num,int den) in new[]{("0.5x",1,2),("0.37x",37,100),("1.6x",8,5),("3x",3,1)}){int w=(source.Width*num+den/2)/den,h=(source.Height*num+den/2)/den;foreach(string mode in new[]{"NearestNeighbor","Bilinear","Bicubic","SuperSampling"}){using var dest=new Surface(w,h);switch(mode){case "NearestNeighbor":dest.NearestNeighborFitSurface(source);break;case "Bilinear":dest.BilinearFitSurface(source);break;case "Bicubic":dest.BicubicFitSurface(source);break;case "SuperSampling":dest.SuperSamplingFitSurface(source);break;}Write(dir,$"{mode}_{label}.png",PngOf(dest),mode,new{scale=label,width=w,height=h},input);}}SaveManifest(dir);}
    // The M03 gate runs every effect on these 5 fixtures; alpha_edge_64 keeps straight color under zero alpha.
    private static readonly string[] EffectFixtures={"impulse_64","partial_alpha_64","plasma_512x384","plasma_alpha_512x384","alpha_edge_64"};
    private static void Flood()
    {
        string dir=Path.Combine(Oracle,"flood");Directory.CreateDirectory(dir);
        foreach((string name,int x,int y) in new[]{("impulse_64",32,32),("partial_alpha_64",32,32),("plasma_512x384",256,192),("plasma_alpha_512x384",256,192),("alpha_edge_64",32,32)})
        {
            string input=F(name);using var surface=SurfaceOf(Png.Read(input));
            foreach(int percent in new[]{0,25,50,100})foreach(bool contiguous in new[]{true,false})
                FloodCase(dir,input,surface,name,x,y,percent,contiguous);
        }
        var threshold=new Png(5,1);
        threshold.Set(0,0,0,0,0,255);threshold.Set(1,0,128,0,0,255);threshold.Set(2,0,129,0,0,255);
        threshold.Set(3,0,200,0,0,0);threshold.Set(4,0,0,200,0,0);
        string thresholdInput=Path.Combine(dir,"threshold_input.png");threshold.Write(thresholdInput);
        using(var surface=SurfaceOf(threshold))
            foreach(bool contiguous in new[]{true,false})FloodCase(dir,thresholdInput,surface,"threshold",0,0,50,contiguous);
        SaveManifest(dir);
    }
    private static void FloodCase(string dir,string input,Surface surface,string name,int x,int y,int percent,bool contiguous)
    {
        int t=(int)((percent/100.0)*(percent/100.0)*256);
        var bits=new PaintDotNet.FloodBits(surface.Width,surface.Height);
        Rectangle bounds;
        if(contiguous)PaintDotNet.Tools.FloodExtract.FillStencilFromPoint(surface,bits,new Point(x,y),t,out bounds,null,false);
        else PaintDotNet.Tools.FloodExtract.FillStencilByColor(surface,bits,surface[x,y],t,out bounds,null,false);
        byte[] mask=new byte[surface.Width*surface.Height];
        for(int py=0;py<surface.Height;py++)for(int px=0;px<surface.Width;px++)mask[py*surface.Width+px]=bits.GetUnchecked(px,py)?(byte)255:(byte)0;
        string path=Path.Combine(dir,$"{name}_{percent}_{(contiguous?"contiguous":"global")}.bin");
        File.WriteAllBytes(path,mask);
        Record(dir,"FloodToolBase",new{seed=new[]{x,y},tolerance_percent=percent,t,contiguous,width=surface.Width,height=surface.Height},input,path);
    }
    private static readonly string[] EffectNames={"Ink Sketch","Oil Painting","Pencil Sketch","Edge Detect","Emboss","Outline","Relief","Invert Colors","Desaturate","Sepia","Brightness and Contrast","Hue and Saturation","Posterize","Auto-Level","Levels","Curves","Fragment Blur","Gaussian Blur","Median Blur","Motion Blur","Radial Blur","Zoom Blur","Add Noise","Frosted Glass","Clouds","Bulge","Pixelate","Polar Inversion","Tile Reflection","Twist","Reduce Noise","Glow","Red Eye Removal","Sharpen","Soften Portrait"};
    private static void EffectsAll()
    {
        string dir=Path.Combine(Oracle,"effects");Directory.CreateDirectory(dir);
        foreach(string old in Directory.GetFiles(dir,"*.png"))File.Delete(old);
        entries.Clear();
        string sourceDir=Path.Combine(Environment.GetEnvironmentVariable("OpenPdnSourceRoot")??Path.Combine(Root,".pdn-src"),"src/Effects");
        var represented=new HashSet<string>(StringComparer.Ordinal){"InvertColorsEffect","DesaturateEffect","SepiaEffect","BrightnessAndContrastAdjustment","HueAndSaturationAdjustment","PosterizeAdjustment","AutoLevelEffect","LevelsEffect","CurvesEffect","FragmentEffect","GaussianBlurEffect","MedianEffect","MotionBlurEffect","RadialBlurEffect","ZoomBlurEffect","AddNoiseEffect","FrostedGlassEffect","CloudsEffect","InkSketchEffect","OilPaintingEffect","PencilSketchEffect","EdgeDetectEffect","EmbossEffect","OutlineEffect","ReliefEffect","BulgeEffect","PixelateEffect","PolarInversionEffect","TileEffect","TwistEffect","ReduceNoiseEffect","GlowEffect","RedEyeRemoveEffect","SharpenEffect","SoftenPortraitEffect"};
        var abstractHelpers=new HashSet<string>(StringComparer.Ordinal){"InternalPropertyBasedEffect","PropertyBasedEffect","ConvolutionFilterEffect","LocalHistogramEffect"};
        var names=EffectNames.Concat(Directory.EnumerateFiles(sourceDir,"*Effect.cs")
            .Select(Path.GetFileNameWithoutExtension).Where(n=>n!="Effect"&&!represented.Contains(n)&&!abstractHelpers.Contains(n)))
            .Distinct().OrderBy(n=>n,StringComparer.Ordinal);
        foreach(string name in names)
        {
            Effects(name,"{}",false);
            if(name=="Hue and Saturation")
            {
                foreach((string label,string settings) in new[]{
                    ("hue_min","{\"hue\":-180}"),("hue_max","{\"hue\":180}"),("hue_mid","{\"hue\":45}"),
                    ("saturation_min","{\"saturation\":0}"),("saturation_max","{\"saturation\":200}"),("saturation_mid","{\"saturation\":150}"),
                    ("lightness_min","{\"lightness\":-100}"),("lightness_max","{\"lightness\":100}"),("lightness_mid","{\"lightness\":50}")})
                    Effects(name,settings,false,label);
            }
            if(name=="Brightness and Contrast")
            {
                foreach((string label,string settings) in new[]{
                    ("brightness_min","{\"brightness\":-100}"),("brightness_max","{\"brightness\":100}"),("brightness_mid","{\"brightness\":50}"),
                    ("contrast_min","{\"contrast\":-100}"),("contrast_max","{\"contrast\":100}"),("contrast_mid","{\"contrast\":50}")})
                    Effects(name,settings,false,label);
            }
            if(name=="Posterize")
            {
                foreach((string label,string settings) in new[]{
                    ("red_min","{\"red\":2}"),("red_max","{\"red\":64}"),("red_mid","{\"red\":32}"),
                    ("green_min","{\"green\":2}"),("green_max","{\"green\":64}"),("green_mid","{\"green\":32}"),
                    ("blue_min","{\"blue\":2}"),("blue_max","{\"blue\":64}"),("blue_mid","{\"blue\":32}")})
                    Effects(name,settings,false,label);
            }
            if(name=="Gaussian Blur")
            {
                foreach((string label,string settings) in new[]{("radius_min","{\"radius\":0}"),("radius_max","{\"radius\":200}"),("radius_mid","{\"radius\":100}")})
                    Effects(name,settings,false,label);
            }
            if(name=="Fragment Blur")
                foreach((string label,string settings) in new[]{("distance_min","{\"distance\":0}"),("fragments_max","{\"fragments\":50}"),("rotation_max","{\"rotation\":360}")})
                    Effects(name,settings,false,label);
            if(name=="Median Blur")
                foreach((string label,string settings) in new[]{("radius_min","{\"radius\":1}"),("percentile_min","{\"percentile\":0}"),("percentile_max","{\"percentile\":100}")})
                    Effects(name,settings,false,label);
            if(name=="Motion Blur")
                foreach((string label,string settings) in new[]{("distance_min","{\"distance\":1}"),("angle_min","{\"angle\":-180}"),("angle_max","{\"angle\":180}"),("uncentered","{\"centered\":false}")})
                    Effects(name,settings,false,label);
            if(name=="Radial Blur")
                foreach((string label,string settings) in new[]{("angle_min","{\"angle\":0}"),("quality_min","{\"quality\":1}"),("offset_mid","{\"offset_x\":0.5,\"offset_y\":-0.5}")})
                    Effects(name,settings,false,label);
            if(name=="Zoom Blur")
                foreach((string label,string settings) in new[]{("amount_min","{\"amount\":0}"),("amount_max","{\"amount\":100}"),("offset_mid","{\"offset_x\":0.5,\"offset_y\":-0.5}")})
                    Effects(name,settings,false,label);
            if(name=="Levels")
            {
                foreach((string label,string settings) in new[]{("gamma_min","{\"gamma\":0.1}"),("gamma_max","{\"gamma\":10}"),("gamma_mid","{\"gamma\":2}")})
                    Effects(name,settings,false,label);
            }
            if(name=="Curves")
            {
                Effects(name,"{\"mode\":\"Luminosity\",\"points\":[[[0,0],[64,130],[128,160],[255,255]]]}",false,"luminosity_points");
                Effects(name,"{\"mode\":\"Rgb\",\"points\":[[[0,0],[96,150],[255,255]],[[0,0],[128,90],[255,255]],[[0,0],[192,120],[255,255]]]}",false,"rgb_points");
                Effects(name,"{\"mode\":\"Rgb\",\"points\":[[[0,255],[255,0]],[[0,0],[255,255]],[[0,0],[255,255]]]}",false,"red_endpoints");
            }
        }
        SaveManifest(Path.Combine(Oracle,"effects"));
    }
    private static int IntParam(JsonElement p,string name,int defaultValue)=>p.TryGetProperty(name,out var value)?value.GetInt32():defaultValue;
    private static double DoubleParam(JsonElement p,string name,double defaultValue)=>p.TryGetProperty(name,out var value)?value.GetDouble():defaultValue;
    private static float FloatParam(JsonElement p,string name,float defaultValue)=>p.TryGetProperty(name,out var value)?value.GetSingle():defaultValue;
    private static void Effects(string name,string json,bool save=true,string variant="default")
    {
        using var doc=JsonDocument.Parse(json);
        string dir=Path.Combine(Oracle,"effects");Directory.CreateDirectory(dir);
        string manifest=Path.Combine(dir,"manifest.json");
        if(save&&entries.Count==0&&File.Exists(manifest))entries=JsonSerializer.Deserialize<List<Entry>>(File.ReadAllText(manifest))??new();
        bool supported=name is "Invert Colors" or "Desaturate" or "Sepia" or "Hue and Saturation" or "Auto-Level" or "Levels" or "Curves" or "Brightness and Contrast" or "Posterize" or "Fragment Blur" or "Gaussian Blur" or "Median Blur" or "Motion Blur" or "Radial Blur" or "Zoom Blur" or "Add Noise" or "Frosted Glass" or "Clouds" or "Ink Sketch" or "Oil Painting" or "Pencil Sketch" or "Edge Detect" or "Emboss" or "Outline" or "Relief" or "Bulge" or "Pixelate" or "Polar Inversion" or "Tile Reflection" or "Twist" or "Reduce Noise" or "Glow" or "Red Eye Removal" or "Sharpen" or "Soften Portrait";
        foreach(string fixture in EffectFixtures)
        {
            string input=F(fixture);
            if(!supported)
            {
                string reason=name switch {
                    "ColorFillEffect"=>"Source class is disabled by #if false.",
                    "SurfaceBlurEffect"=>"Source file is excluded from the OpenPDN 3.36 Effects project.",
                    "BlurEffect"=>"Obsolete compatibility wrapper; the Gaussian Blur effect is rendered directly.",
                    _=>"No compile entry or CLI dispatcher exists for this effect class."
                };
                Record(dir,name,doc.RootElement.Clone(),input,"","not-run",reason);
                continue;
            }
            var src=Png.Read(input);
            if(name is "Brightness and Contrast" or "Posterize" or "Fragment Blur" or "Gaussian Blur" or "Median Blur" or "Motion Blur" or "Radial Blur" or "Zoom Blur" or "Add Noise" or "Frosted Glass" or "Clouds" or "Ink Sketch" or "Oil Painting" or "Pencil Sketch" or "Edge Detect" or "Emboss" or "Outline" or "Relief" or "Bulge" or "Pixelate" or "Polar Inversion" or "Tile Reflection" or "Twist" or "Reduce Noise" or "Glow" or "Red Eye Removal" or "Sharpen" or "Soften Portrait")
            {
                using var sourceSurface=SurfaceOf(src);
                using var destSurface=new Surface(src.Width,src.Height);
                InternalPropertyBasedEffect effect=name switch {
                    "Brightness and Contrast"=>new BrightnessAndContrastAdjustment(),
                    "Posterize"=>new PosterizeAdjustment(),
                    "Gaussian Blur"=>new GaussianBlurEffect(),
                    "Fragment Blur"=>new FragmentEffect(),
                    "Median Blur"=>new MedianEffect(),
                    "Motion Blur"=>new MotionBlurEffect(),
                    "Radial Blur"=>new RadialBlurEffect(),
                    "Zoom Blur"=>new ZoomBlurEffect(),
                    "Add Noise"=>new AddNoiseEffect(),
                    "Reduce Noise"=>new ReduceNoiseEffect(),
                    "Glow"=>new GlowEffect(),
                    "Red Eye Removal"=>new RedEyeRemoveEffect(),
                    "Sharpen"=>new SharpenEffect(),
                    "Soften Portrait"=>new SoftenPortraitEffect(),
                    "Frosted Glass"=>new FrostedGlassEffect(),
                    "Ink Sketch"=>new InkSketchEffect(),
                    "Oil Painting"=>new OilPaintingEffect(),
                    "Pencil Sketch"=>new PencilSketchEffect(),
                    "Edge Detect"=>new EdgeDetectEffect(),
                    "Emboss"=>new EmbossEffect(),
                    "Outline"=>new OutlineEffect(),
                    "Relief"=>new ReliefEffect(),
                    "Bulge"=>new BulgeEffect(),
                    "Pixelate"=>new PixelateEffect(),
                    "Polar Inversion"=>new PolarInversionEffect(),
                    "Tile Reflection"=>new TileEffect(),
                    "Twist"=>new TwistEffect(),
                    _=>new CloudsEffect()
                };
                var token=effect.CreateToken();
                if(effect is BrightnessAndContrastAdjustment)
                {
                    token.GetProperty<Int32Property>(BrightnessAndContrastAdjustment.PropertyNames.Brightness).Value=IntParam(doc.RootElement,"brightness",0);
                    token.GetProperty<Int32Property>(BrightnessAndContrastAdjustment.PropertyNames.Contrast).Value=IntParam(doc.RootElement,"contrast",0);
                }
                if(effect is PosterizeAdjustment)
                {
                    token.GetProperty<Int32Property>(PosterizeAdjustment.PropertyNames.RedLevels).Value=IntParam(doc.RootElement,"red",16);
                    token.GetProperty<Int32Property>(PosterizeAdjustment.PropertyNames.GreenLevels).Value=IntParam(doc.RootElement,"green",16);
                    token.GetProperty<Int32Property>(PosterizeAdjustment.PropertyNames.BlueLevels).Value=IntParam(doc.RootElement,"blue",16);
                }
                if(effect is GaussianBlurEffect)
                    token.GetProperty<Int32Property>(GaussianBlurEffect.PropertyNames.Radius).Value=IntParam(doc.RootElement,"radius",2);
                if(effect is FragmentEffect)
                {
                    token.GetProperty<Int32Property>(FragmentEffect.PropertyNames.Fragments).Value=IntParam(doc.RootElement,"fragments",4);
                    token.GetProperty<Int32Property>(FragmentEffect.PropertyNames.Distance).Value=IntParam(doc.RootElement,"distance",8);
                    token.GetProperty<DoubleProperty>(FragmentEffect.PropertyNames.Rotation).Value=DoubleParam(doc.RootElement,"rotation",0);
                }
                if(effect is MedianEffect)
                {
                    token.GetProperty<Int32Property>(MedianEffect.PropertyNames.Radius).Value=IntParam(doc.RootElement,"radius",10);
                    token.GetProperty<Int32Property>(MedianEffect.PropertyNames.Percentile).Value=IntParam(doc.RootElement,"percentile",50);
                }
                if(effect is MotionBlurEffect)
                {
                    token.GetProperty<DoubleProperty>(MotionBlurEffect.PropertyNames.Angle).Value=DoubleParam(doc.RootElement,"angle",25);
                    token.GetProperty<Int32Property>(MotionBlurEffect.PropertyNames.Distance).Value=IntParam(doc.RootElement,"distance",10);
                    token.GetProperty<BooleanProperty>(MotionBlurEffect.PropertyNames.Centered).Value=doc.RootElement.TryGetProperty("centered",out var centered)?centered.GetBoolean():true;
                }
                if(effect is RadialBlurEffect)
                {
                    token.GetProperty<DoubleProperty>(RadialBlurEffect.PropertyNames.Angle).Value=DoubleParam(doc.RootElement,"angle",2);
                    token.GetProperty<Int32Property>(RadialBlurEffect.PropertyNames.Quality).Value=IntParam(doc.RootElement,"quality",2);
                    token.GetProperty<DoubleVectorProperty>(RadialBlurEffect.PropertyNames.Offset).Value=Pair.Create(DoubleParam(doc.RootElement,"offset_x",0),DoubleParam(doc.RootElement,"offset_y",0));
                }
                if(effect is ZoomBlurEffect)
                {
                    token.GetProperty<Int32Property>(ZoomBlurEffect.PropertyNames.Amount).Value=IntParam(doc.RootElement,"amount",10);
                    token.GetProperty<DoubleVectorProperty>(ZoomBlurEffect.PropertyNames.Offset).Value=Pair.Create(DoubleParam(doc.RootElement,"offset_x",0),DoubleParam(doc.RootElement,"offset_y",0));
                }
                if(effect is AddNoiseEffect)
                {
                    token.GetProperty<Int32Property>(AddNoiseEffect.PropertyNames.Intensity).Value=IntParam(doc.RootElement,"intensity",64);
                    token.GetProperty<Int32Property>(AddNoiseEffect.PropertyNames.Saturation).Value=IntParam(doc.RootElement,"saturation",100);
                    token.GetProperty<DoubleProperty>(AddNoiseEffect.PropertyNames.Coverage).Value=DoubleParam(doc.RootElement,"coverage",100);
                }
                if(effect is ReduceNoiseEffect)
                {
                    token.GetProperty<Int32Property>(ReduceNoiseEffect.PropertyNames.Radius).Value=IntParam(doc.RootElement,"radius",10);
                    token.GetProperty<DoubleProperty>(ReduceNoiseEffect.PropertyNames.Strength).Value=DoubleParam(doc.RootElement,"strength",0.4);
                }
                if(effect is GlowEffect)
                {
                    token.GetProperty<Int32Property>(GlowEffect.PropertyNames.Radius).Value=IntParam(doc.RootElement,"radius",6);
                    token.GetProperty<Int32Property>(GlowEffect.PropertyNames.Brightness).Value=IntParam(doc.RootElement,"brightness",10);
                    token.GetProperty<Int32Property>(GlowEffect.PropertyNames.Contrast).Value=IntParam(doc.RootElement,"contrast",10);
                }
                if(effect is RedEyeRemoveEffect)
                {
                    token.GetProperty<Int32Property>(RedEyeRemoveEffect.PropertyNames.Tolerance).Value=IntParam(doc.RootElement,"tolerance",70);
                    token.GetProperty<Int32Property>(RedEyeRemoveEffect.PropertyNames.Saturation).Value=IntParam(doc.RootElement,"saturation",90);
                }
                if(effect is SharpenEffect)
                    token.GetProperty<Int32Property>(SharpenEffect.PropertyNames.Amount).Value=IntParam(doc.RootElement,"amount",2);
                if(effect is SoftenPortraitEffect)
                {
                    token.GetProperty<Int32Property>(SoftenPortraitEffect.PropertyNames.Softness).Value=IntParam(doc.RootElement,"softness",5);
                    token.GetProperty<Int32Property>(SoftenPortraitEffect.PropertyNames.Lighting).Value=IntParam(doc.RootElement,"lighting",0);
                    token.GetProperty<Int32Property>(SoftenPortraitEffect.PropertyNames.Warmth).Value=IntParam(doc.RootElement,"warmth",10);
                }
                if(effect is FrostedGlassEffect)
                {
                    token.GetProperty<DoubleProperty>(FrostedGlassEffect.PropertyNames.MinScatterRadius).Value=DoubleParam(doc.RootElement,"minRadius",0);
                    token.GetProperty<DoubleProperty>(FrostedGlassEffect.PropertyNames.MaxScatterRadius).Value=DoubleParam(doc.RootElement,"maxRadius",3);
                    token.GetProperty<Int32Property>(FrostedGlassEffect.PropertyNames.NumSamples).Value=IntParam(doc.RootElement,"samples",2);
                }
                if(effect is BulgeEffect)
                {
                    token.GetProperty<Int32Property>(BulgeEffect.PropertyNames.Amount).Value=IntParam(doc.RootElement,"amount",45);
                    token.GetProperty<DoubleVectorProperty>(BulgeEffect.PropertyNames.Offset).Value=Pair.Create(DoubleParam(doc.RootElement,"center_x",0),DoubleParam(doc.RootElement,"center_y",0));
                }
                if(effect is PixelateEffect)
                    token.GetProperty<Int32Property>(PixelateEffect.PropertyNames.CellSize).Value=IntParam(doc.RootElement,"cell_size",2);
                if(effect is PolarInversionEffect)
                {
                    token.GetProperty<DoubleProperty>(PolarInversionEffect.PropertyNames.Amount).Value=DoubleParam(doc.RootElement,"scale",1);
                    token.GetProperty<DoubleVectorProperty>(PolarInversionEffect.PropertyNames.Offset).Value=Pair.Create(DoubleParam(doc.RootElement,"offset_x",0),DoubleParam(doc.RootElement,"offset_y",0));
                    token.GetProperty<Int32Property>(PolarInversionEffect.PropertyNames.Quality).Value=IntParam(doc.RootElement,"quality",2);
                }
                if(effect is TileEffect)
                {
                    token.GetProperty<DoubleProperty>(TileEffect.PropertyNames.Rotation).Value=DoubleParam(doc.RootElement,"angle",30);
                    token.GetProperty<DoubleProperty>(TileEffect.PropertyNames.SquareSize).Value=DoubleParam(doc.RootElement,"tile_size",40);
                    token.GetProperty<DoubleProperty>(TileEffect.PropertyNames.Curvature).Value=DoubleParam(doc.RootElement,"curvature",8);
                    token.GetProperty<Int32Property>(TileEffect.PropertyNames.Quality).Value=IntParam(doc.RootElement,"quality",2);
                }
                if(effect is TwistEffect)
                {
                    token.GetProperty<DoubleProperty>(TwistEffect.PropertyNames.Amount).Value=DoubleParam(doc.RootElement,"amount",30);
                    token.GetProperty<DoubleProperty>(TwistEffect.PropertyNames.Size).Value=DoubleParam(doc.RootElement,"size",1);
                    token.GetProperty<DoubleVectorProperty>(TwistEffect.PropertyNames.Offset).Value=Pair.Create(DoubleParam(doc.RootElement,"center_x",0),DoubleParam(doc.RootElement,"center_y",0));
                    token.GetProperty<Int32Property>(TwistEffect.PropertyNames.Quality).Value=IntParam(doc.RootElement,"quality",2);
                }
                if(effect is CloudsEffect)
                {
                    token.GetProperty<Int32Property>(CloudsEffect.PropertyNames.Scale).Value=IntParam(doc.RootElement,"scale",250);
                    token.GetProperty<DoubleProperty>(CloudsEffect.PropertyNames.Power).Value=DoubleParam(doc.RootElement,"power",0.5);
                    token.GetProperty<Int32Property>(CloudsEffect.PropertyNames.Seed).Value=IntParam(doc.RootElement,"seed",0);
                }
                if(effect is InkSketchEffect) { token.GetProperty<Int32Property>(InkSketchEffect.PropertyNames.InkOutline).Value=IntParam(doc.RootElement,"ink_outline",50); token.GetProperty<Int32Property>(InkSketchEffect.PropertyNames.Coloring).Value=IntParam(doc.RootElement,"coloring",50); }
                if(effect is OilPaintingEffect) { token.GetProperty<Int32Property>(OilPaintingEffect.PropertyNames.BrushSize).Value=IntParam(doc.RootElement,"brush_size",3); token.GetProperty<Int32Property>(OilPaintingEffect.PropertyNames.Coarseness).Value=IntParam(doc.RootElement,"coarseness",50); }
                if(effect is PencilSketchEffect) { token.GetProperty<Int32Property>(PencilSketchEffect.PropertyNames.PencilTipSize).Value=IntParam(doc.RootElement,"pencil_tip_size",2); token.GetProperty<Int32Property>(PencilSketchEffect.PropertyNames.ColorRange).Value=IntParam(doc.RootElement,"range",0); }
                if(effect is EdgeDetectEffect) token.GetProperty<DoubleProperty>(EdgeDetectEffect.PropertyNames.Angle).Value=DoubleParam(doc.RootElement,"angle",45);
                if(effect is EmbossEffect) token.GetProperty<DoubleProperty>(EmbossEffect.PropertyNames.Angle).Value=DoubleParam(doc.RootElement,"angle",0);
                if(effect is ReliefEffect) token.GetProperty<DoubleProperty>(ReliefEffect.PropertyNames.Angle).Value=DoubleParam(doc.RootElement,"angle",45);
                if(effect is OutlineEffect) { token.GetProperty<Int32Property>(OutlineEffect.PropertyNames.Thickness).Value=IntParam(doc.RootElement,"thickness",3); token.GetProperty<Int32Property>(OutlineEffect.PropertyNames.Intensity).Value=IntParam(doc.RootElement,"intensity",50); }
                effect.Render(token,destSurface,sourceSurface);
                bool patched=name is "Add Noise" or "Frosted Glass" or "Clouds";
                string note=name=="Clouds"?"Instance seed fixed to 0x5a; primary black and secondary white; pixel math unchanged.":patched?"Random seed fixed to 0x51ed270b; pixel math unchanged.":"Direct OpenPDN effect class with UI and property stubs.";
                Write(dir,$"{name.Replace(' ','_')}_{variant}_{fixture}.png",PngOf(destSurface),name,doc.RootElement.Clone(),input,note,patched?"seed-patched":"ok");
                continue;
            }
            var dst=new Png(src.Width,src.Height);
            UnaryPixelOp op=name switch {
                "Invert Colors"=>new UnaryPixelOps.Invert(),
                "Desaturate"=>new UnaryPixelOps.Desaturate(),
                "Hue and Saturation"=>HueOp(doc.RootElement),
                "Levels"=>new UnaryPixelOps.Level(ColorBgra.Black,ColorBgra.White,new float[]{FloatParam(doc.RootElement,"gamma",1),FloatParam(doc.RootElement,"gamma",1),FloatParam(doc.RootElement,"gamma",1)},ColorBgra.Black,ColorBgra.White),
                "Curves"=>CurveOp(doc.RootElement),
                _=>new UnaryPixelOps.Identity()
            };
            if(name=="Auto-Level")
            {
                using var surface=SurfaceOf(src);var histogram=new HistogramRgb();histogram.UpdateHistogram(surface);
                var levels=histogram.MakeLevelsAuto();if(levels.isValid)op=levels;
            }
            var desaturate=new UnaryPixelOps.Desaturate();
            var sepiaLevel=new UnaryPixelOps.Level(ColorBgra.Black,ColorBgra.White,new float[]{1.2f,1.0f,0.8f},ColorBgra.Black,ColorBgra.White);
            for(int y=0;y<src.Height;y++)for(int x=0;x<src.Width;x++)
            {
                var c=Get(src,x,y);c=name=="Sepia"?sepiaLevel.Apply(desaturate.Apply(c)):op.Apply(c);Put(dst,x,y,c);
            }
            Write(dir,$"{name.Replace(' ','_')}_{variant}_{fixture}.png",dst,name,doc.RootElement.Clone(),input,"Original OpenPDN UnaryPixelOps and HistogramRgb; effect setup follows the 3.36 source.");
        }
        if(save)SaveManifest(dir);
    }
    private static UnaryPixelOp CurveOp(JsonElement parameters)
    {
        bool rgb=parameters.TryGetProperty("mode",out var mode)&&mode.GetString()=="Rgb";
        JsonElement points;
        if(!parameters.TryGetProperty("points",out points))
        {
            using var defaults=JsonDocument.Parse("[[[0,0],[255,255]]]");
            points=defaults.RootElement.Clone();
        }
        var op=rgb?new UnaryPixelOps.ChannelCurve():null;
        var lum=rgb?null:new UnaryPixelOps.LuminosityCurve();
        for(int channel=0;channel<points.GetArrayLength();channel++)
        {
            var spline=new SplineInterpolator();
            foreach(var pair in points[channel].EnumerateArray())spline.Add(pair[0].GetDouble(),pair[1].GetDouble());
            byte[] table=rgb?(channel==0?op.CurveR:channel==1?op.CurveG:op.CurveB):lum.Curve;
            for(int value=0;value<256;value++)table[value]=Utility.ClampToByte(spline.Interpolate(value));
        }
        return rgb?op:lum;
    }
    private static UnaryPixelOp HueOp(JsonElement p)
    {
        int hue=IntParam(p,"hue",0),sat=IntParam(p,"saturation",100),light=IntParam(p,"lightness",0);
        if(sat>100)sat=((sat-100)*3)+100;
        return hue==0&&sat==100&&light==0?new UnaryPixelOps.Identity():new UnaryPixelOps.HueSaturationLightness(hue,sat,light);
    }
}
