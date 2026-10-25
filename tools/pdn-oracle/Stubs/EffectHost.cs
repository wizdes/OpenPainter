using System;
using System.Collections.Generic;
using System.Drawing;

namespace System.Windows.Forms { public class Control { } }
namespace PaintDotNet.IndirectUI {
    public enum ControlInfoPropertyNames { DisplayName, Description, UseExponentialScale, SliderLargeChange, SliderSmallChange, UpDownIncrement, ButtonText, SliderSmallChangeX, SliderLargeChangeX, UpDownIncrementX, SliderSmallChangeY, SliderLargeChangeY, UpDownIncrementY, StaticImageUnderlay }
    public enum PropertyControlType { IncrementButton, AngleChooser }
    public sealed class ValueHolder { public object Value {get;set;} }
    public sealed class PropertyControlInfo {
        public ValueHolder ControlType {get;}=new();
        public readonly Dictionary<object,ValueHolder> ControlProperties=new();
        public void SetValueDisplayName(object value,string name) { }
    }
    public sealed class ControlInfo {
        public void SetPropertyControlValue(object name, ControlInfoPropertyNames property, object value) { }
        public void SetPropertyControlType(object name,PropertyControlType type) { }
        public PropertyControlInfo FindControlForPropertyName(object name)=>new();
    }
}
namespace PaintDotNet.PropertySystem {
    public abstract class Property { public object Name { get; } protected Property(object name) { Name=name; } public object Value { get=>GetType().GetProperty("Value",System.Reflection.BindingFlags.DeclaredOnly|System.Reflection.BindingFlags.Public|System.Reflection.BindingFlags.Instance)!.GetValue(this); set=>GetType().GetProperty("Value",System.Reflection.BindingFlags.DeclaredOnly|System.Reflection.BindingFlags.Public|System.Reflection.BindingFlags.Instance)!.SetValue(this,value); } }
    public sealed class Int32Property : Property { public new int Value { get; set; } public Int32Property(object name,int value,int min,int max):base(name){Value=value;} }
    public sealed class DoubleProperty : Property { public new double Value {get;set;} public DoubleProperty(object name,double value,double min,double max):base(name){Value=value;} }
    public sealed class StaticListChoiceProperty : Property { public new object Value {get;set;} public StaticListChoiceProperty(object name,Type[] values,int index,bool allowCustom):base(name){Value=values[index];} }
    public sealed class BooleanProperty : Property { public new bool Value { get; set; } public BooleanProperty(object name,bool value):base(name){Value=value;} }
    public sealed class DoubleVectorProperty : Property { public double ValueX {get;set;} public double ValueY {get;set;} public new PaintDotNet.Pair<double,double> Value {get=>PaintDotNet.Pair.Create(ValueX,ValueY);set{ValueX=value.First;ValueY=value.Second;}} public DoubleVectorProperty(object name,PaintDotNet.Pair<double,double> value,PaintDotNet.Pair<double,double> min,PaintDotNet.Pair<double,double> max):base(name){Value=value;} }
    public abstract class PropertyCollectionRule { }
    public sealed class SoftMutuallyBoundMinMaxRule<TValue,TProperty> : PropertyCollectionRule { public SoftMutuallyBoundMinMaxRule(object min,object max) { } }
    public sealed class LinkValuesBasedOnBooleanRule<TValue,TProperty> : PropertyCollectionRule {
        public LinkValuesBasedOnBooleanRule(object[] names,object link,bool inverse) { }
    }
    public sealed class PropertyCollection {
        public readonly Dictionary<object,Property> Values=new();
        public Property this[object name]=>Values[name];
        public PropertyCollection Clone()=>this;
        public PropertyCollection(IEnumerable<Property> values) { foreach(var p in values)Values[p.Name]=p; }
        public PropertyCollection(IEnumerable<Property> values,IEnumerable<PropertyCollectionRule> rules):this(values) { }
        public static PropertyCollection CreateEmpty()=>new(Array.Empty<Property>());
    }
}
namespace PaintDotNet {
    public readonly record struct Pair<T1,T2>(T1 First,T2 Second);
    public static class Pair { public static Pair<T1,T2> Create<T1,T2>(T1 first,T2 second)=>new(first,second); }
    public static partial class PdnResources {
        public static ImageResource GetImageResource(string name)=>new();
    }
    public sealed class ImageResource { public Image Reference=>null; public static ImageResource FromImage(Image image)=>new(); }
    public sealed class RenderArgs { public Surface Surface {get;} public int Width=>Surface.Width; public int Height=>Surface.Height; public RenderArgs(Surface surface){Surface=surface;} }
}
namespace PaintDotNet.Effects {
    using PaintDotNet.IndirectUI;
    using PaintDotNet.PropertySystem;
    [Flags] public enum EffectFlags { None=0, Configurable=1 }
    public static class SubmenuNames { public const string Blurs="Blurs"; public const string Distort="Distort"; public const string Render="Render"; public const string Noise="Noise"; public const string Photo="Photo"; }
    public enum EffectCategory { Adjustment }
    [AttributeUsage(AttributeTargets.Class)] public sealed class EffectCategoryAttribute : Attribute {
        public EffectCategoryAttribute(EffectCategory category) { }
    }
    public sealed class PropertyBasedEffectConfigToken {
        private readonly PropertyCollection properties;
        public PropertyBasedEffectConfigToken(PropertyCollection properties){this.properties=properties;}
        public T GetProperty<T>(object name) where T:Property=>(T)properties.Values[name];
        public void SetPropertyValue(object name,object value)=>properties.Values[name].Value=value;
    }
    public abstract class PropertyBasedEffect {
        public static ControlInfo CreateDefaultConfigUI(PropertyCollection properties)=>new();
    }
    public sealed class EffectEnvironmentParameters { public PaintDotNet.ColorBgra PrimaryColor {get;}=PaintDotNet.ColorBgra.Black; public PaintDotNet.ColorBgra SecondaryColor {get;}=PaintDotNet.ColorBgra.White; public PaintDotNet.Surface SourceSurface {get;}=null; }
    public abstract class InternalPropertyBasedEffect : PropertyBasedEffect {
        protected RenderArgs DstArgs {get;private set;}
        protected RenderArgs SrcArgs {get;private set;}
        protected EffectEnvironmentParameters EnvironmentParameters {get;}=new();
        protected InternalPropertyBasedEffect(string name,Image image,string submenu,EffectFlags flags) { }
        protected abstract PropertyCollection OnCreatePropertyCollection();
        protected virtual ControlInfo OnCreateConfigUI(PropertyCollection properties)=>CreateDefaultConfigUI(properties);
        protected virtual void OnSetRenderInfo(PropertyBasedEffectConfigToken token,RenderArgs dst,RenderArgs src){DstArgs=dst;SrcArgs=src;}
        protected abstract void OnRender(Rectangle[] rois,int startIndex,int length);
        public PropertyBasedEffectConfigToken CreateToken()=>new(OnCreatePropertyCollection());
        public PropertyCollection CreatePropertyCollection()=>OnCreatePropertyCollection();
        public void SetRenderInfo(PropertyBasedEffectConfigToken token,RenderArgs dst,RenderArgs src)=>OnSetRenderInfo(token,dst,src);
        public void Render(Rectangle[] rois,int startIndex,int length)=>OnRender(rois,startIndex,length);
        public void Render(PropertyBasedEffectConfigToken token,Surface dst,Surface src){OnSetRenderInfo(token,new(dst),new(src));OnRender(new[]{dst.Bounds},0,1);}
    }
}
