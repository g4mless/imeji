using SkiaSharp;

namespace ImejiWinForms.Models;


public class ImageState
{
    public SKBitmap? Image { get; set; }
    public string? Filename { get; set; }
    public float Zoom { get; set; } = 1.0f;
    public SKPoint PanOffset { get; set; } = SKPoint.Empty;
    
    // Animation state
    public bool IsAnimating { get; set; }
    public DateTime AnimationStartTime { get; set; }
    public SKPoint AnimationStartOffset { get; set; }
    
    // Drag state
    public bool IsDragging { get; set; }
    public SKPoint LastMousePos { get; set; }


    public void Reset()
    {
        Image?.Dispose();
        Image = null;
        Filename = null;
        Zoom = 1.0f;
        PanOffset = SKPoint.Empty;
        IsAnimating = false;
        IsDragging = false;
    }


    public void ResetTransform()
    {
        Zoom = 1.0f;
        PanOffset = SKPoint.Empty;
        IsAnimating = false;
        IsDragging = false;
    }
}
