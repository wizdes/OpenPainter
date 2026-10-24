using System;
using System.Drawing;

namespace PaintDotNet {
    internal interface IBitVector2D {
        int Width { get; }
        int Height { get; }
        void Clear(bool value);
        void Set(Rectangle rect, bool value);
        void SetUnchecked(int x,int y,bool value);
        bool GetUnchecked(int x,int y);
    }
    internal sealed class FloodBits : IBitVector2D {
        private readonly bool[] bits;
        public int Width { get; }
        public int Height { get; }
        public FloodBits(int width,int height) { Width=width;Height=height;bits=new bool[width*height]; }
        public void Clear(bool value)=>Array.Fill(bits,value);
        public void Set(Rectangle rect,bool value) { for(int y=rect.Top;y<rect.Bottom;y++)for(int x=rect.Left;x<rect.Right;x++)bits[y*Width+x]=value; }
        public void SetUnchecked(int x,int y,bool value)=>bits[y*Width+x]=value;
        public bool GetUnchecked(int x,int y)=>bits[y*Width+x];
    }
}
