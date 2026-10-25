// Original: src/Core/Utility.cs at cca476b0df2a2f70996e6b9486ec45327631568c
// MIT License, Copyright dotPDN LLC
// Changed: kept byte math and numeric helpers verbatim; removed Windows UI methods.
using System; using System.Drawing; using System.IO;
namespace PaintDotNet { public static class Utility {
        // Original OpenPDN Utility.GetRgssOffsets at cca476b, MIT.
        public static unsafe void GetRgssOffsets(PointF* samplesArray, int sampleCount, int quality)
        {
            if (sampleCount < 1 || sampleCount != quality * quality) throw new ArgumentOutOfRangeException(nameof(sampleCount));
            if (sampleCount == 1) { samplesArray[0] = new PointF(0.0f, 0.0f); return; }
            for (int i = 0; i < sampleCount; ++i)
            {
                double y = (i + 1d) / (sampleCount + 1d);
                double x = y * quality;
                x -= (int)x;
                samplesArray[i] = new PointF((float)(x - 0.5d), (float)(y - 0.5d));
            }
        }

        // TODO: In v4.0 codebase, turn this into an extension method
        public static double Clamp(double x, double min, double max)
        {
            if (x < min)
            {
                return min;
            }
            else if (x > max)
            {
                return max;
            }
            else
            {
                return x;
            }
        }

        // TODO: In v4.0 codebase, turn this into an extension method
        public static float Clamp(float x, float min, float max)
        {
            if (x < min)
            {
                return min;
            }
            else if (x > max)
            {
                return max;
            }
            else
            {
                return x;
            }
        }

        // TODO: In v4.0 codebase, turn this into an extension method
        public static int Clamp(int x, int min, int max)
        {
            if (x < min)
            {
                return min;
            }
            else if (x > max)
            {
                return max;
            }
            else
            {
                return x;
            }
        }

        public static byte ClampToByte(double x)
        {
            if (x > 255)
            {
                return 255;
            }
            else if (x < 0)
            {
                return 0;
            }
            else
            {
                return (byte)x;
            }
        }

        public static byte ClampToByte(float x)
        {
            if (x > 255)
            {
                return 255;
            }
            else if (x < 0)
            {
                return 0;
            }
            else
            {
                return (byte)x;
            }
        }

        public static byte ClampToByte(int x)
        {
            if (x > 255)
            {
                return 255;
            }
            else if (x < 0)
            {
                return 0;
            }
            else
            {
                return (byte)x;
            }
        }

        public static float Lerp(float from, float to, float frac)
        {
            return (from + frac * (to - from));
        }

        public static double Lerp(double from, double to, double frac)
        {
            return (from + frac * (to - from));
        }

        public static PointF Lerp(PointF from, PointF to, float frac)
        {
            return new PointF(Lerp(from.X, to.X, frac), Lerp(from.Y, to.Y, frac));
        }

        public static byte FastScaleByteByByte(byte a, byte b)
        {
            int r1 = a * b + 0x80;
            int r2 = ((r1 >> 8) + r1) >> 8;
            return (byte)r2;
        }

internal static bool IsNumber(float x)=>!float.IsNaN(x)&&!float.IsInfinity(x);
internal static bool IsNumber(double x)=>!double.IsNaN(x)&&!double.IsInfinity(x);
public static void GCFullCollect()=>GC.Collect();
public static void ReadFromStream(Stream s,byte[] b,int o,int n){s.ReadExactly(b.AsSpan(o,n));}
public static Rectangle GetRegionBounds(Rectangle[] r,int start,int length){Rectangle result=Rectangle.Empty;for(int i=start;i<start+length;i++)result=Rectangle.Union(result,r[i]);return result;}
public static RectangleF GetRegionBounds(RectangleF[] r,int start,int length){RectangleF result=RectangleF.Empty;for(int i=start;i<start+length;i++)result=RectangleF.Union(result,r[i]);return result;}
public static string GetStaticName(Type type)=>type.Name;
} }
