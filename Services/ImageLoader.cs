using SkiaSharp;
using System;
using System.IO;

namespace ImejiWinForms.Services;


public static class ImageLoader
{

    public static (SKBitmap? bitmap, string? error) LoadFromFile(string filePath)
    {
        try
        {
            if (!File.Exists(filePath))
            {
                return (null, "File not found");
            }

            var bitmap = SKBitmap.Decode(filePath);
            if (bitmap == null)
            {
                return (null, "Failed to decode image");
            }

            return (bitmap, null);
        }
        catch (Exception ex)
        {
            return (null, $"Error loading image: {ex.Message}");
        }
    }


    public static (SKBitmap? bitmap, string? error) LoadFromBytes(byte[] bytes)
    {
        try
        {
            var bitmap = SKBitmap.Decode(bytes);
            if (bitmap == null)
            {
                return (null, "Failed to decode image");
            }

            return (bitmap, null);
        }
        catch (Exception ex)
        {
            return (null, $"Error loading image: {ex.Message}");
        }
    }


    public static (SKBitmap? bitmap, string? error) LoadFromStream(Stream stream)
    {
        try
        {
            var bitmap = SKBitmap.Decode(stream);
            if (bitmap == null)
            {
                return (null, "Failed to decode image");
            }

            return (bitmap, null);
        }
        catch (Exception ex)
        {
            return (null, $"Error loading image: {ex.Message}");
        }
    }
}
