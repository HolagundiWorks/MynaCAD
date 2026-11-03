using System.Runtime.CompilerServices;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Windows.Foundation;
using WinRT.Interop;

namespace Aad.WinUi;

/// <summary>How a secondary window chooses its size.</summary>
internal enum ShellFitMode
{
    /// <summary>Size to measured content on open; not user-resized.</summary>
    Content,
    /// <summary>Size to content when asked; may grow/shrink with content until the user resizes.</summary>
    Adaptive,
    /// <summary>Sensible initial content size; after the user resizes, keep their size (optionally on disk).</summary>
    UserResizable,
}

/// <summary>
/// Content-based sizing for dialogs and tool windows. Does not touch the main CAD window.
/// Uses measured DesiredSize + non-client overhead, clamped to min/max and the monitor work area.
/// </summary>
internal static class ShellFit
{
    private static readonly ConditionalWeakTable<Window, State> States = new();

    public static void ToContent(Window window, double minWidthDip = 280, double minHeightDip = 120, double maxWidthDip = 640, double maxHeightDip = 480) =>
        Attach(window, new Options
        {
            Mode = ShellFitMode.Content,
            MinWidthDip = minWidthDip,
            MinHeightDip = minHeightDip,
            MaxWidthDip = maxWidthDip,
            MaxHeightDip = maxHeightDip,
        });

    public static void Adaptive(Window window, double minWidthDip = 320, double minHeightDip = 160, double maxWidthDip = 720, double maxHeightDip = 640) =>
        Attach(window, new Options
        {
            Mode = ShellFitMode.Adaptive,
            MinWidthDip = minWidthDip,
            MinHeightDip = minHeightDip,
            MaxWidthDip = maxWidthDip,
            MaxHeightDip = maxHeightDip,
        });

    public static void UserResizable(
        Window window,
        double minWidthDip = 360,
        double minHeightDip = 240,
        double maxWidthDip = 1200,
        double maxHeightDip = 900,
        string? persistKey = null) =>
        Attach(window, new Options
        {
            Mode = ShellFitMode.UserResizable,
            MinWidthDip = minWidthDip,
            MinHeightDip = minHeightDip,
            MaxWidthDip = maxWidthDip,
            MaxHeightDip = maxHeightDip,
            PersistKey = persistKey,
        });

    public static void Attach(Window window, Options options)
    {
        if (ShellWindows.IsMain(window))
        {
            return;
        }
        var state = States.GetValue(window, _ => new State());
        state.Options = options;
        state.UserSized = false;
        if (window.AppWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.IsResizable = options.Mode != ShellFitMode.Content;
        }
        if (window.Content is FrameworkElement root)
        {
            void OnLoaded(object sender, RoutedEventArgs e)
            {
                root.Loaded -= OnLoaded;
                if (!TryRestore(window, state))
                {
                    Apply(window, state, force: true);
                }
                if (options.Mode == ShellFitMode.Adaptive)
                {
                    // Only re-fit when content grows/shrinks by a real amount — not our own Resize.
                    root.SizeChanged += (_, args) =>
                    {
                        if (state.Applying || state.UserSized)
                        {
                            return;
                        }
                        if (Math.Abs(args.PreviousSize.Width - args.NewSize.Width) < 1
                            && Math.Abs(args.PreviousSize.Height - args.NewSize.Height) < 1)
                        {
                            return;
                        }
                        Queue(window, state);
                    };
                }
            }
            if (root.IsLoaded)
            {
                OnLoaded(root, new RoutedEventArgs());
            }
            else
            {
                root.Loaded += OnLoaded;
            }
        }
        window.AppWindow.Changed -= state.OnAppChanged;
        state.OnAppChanged = (_, args) =>
        {
            if (state.Applying || !args.DidSizeChange)
            {
                return;
            }
            if (options.Mode is ShellFitMode.UserResizable or ShellFitMode.Adaptive)
            {
                state.UserSized = true;
                QueuePersist(window, state);
            }
        };
        window.AppWindow.Changed += state.OnAppChanged;
        window.Closed += (_, _) =>
        {
            window.AppWindow.Changed -= state.OnAppChanged;
            state.Timer?.Stop();
            state.PersistTimer?.Stop();
            States.Remove(window);
        };
    }

    /// <summary>Re-measure (Adaptive / Content). No-op after the user resized a UserResizable window.</summary>
    public static void Invalidate(Window window)
    {
        if (!States.TryGetValue(window, out var state))
        {
            return;
        }
        if (state.Options.Mode == ShellFitMode.UserResizable && state.UserSized)
        {
            return;
        }
        state.UserSized = false;
        Queue(window, state);
    }

    private static void Queue(Window window, State state)
    {
        if (state.Applying || (state.Options.Mode == ShellFitMode.UserResizable && state.UserSized))
        {
            return;
        }
        state.Timer ??= new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(50) };
        state.Timer.Stop();
        state.Timer.Tick -= state.Tick;
        state.Tick = (_, _) =>
        {
            state.Timer?.Stop();
            Apply(window, state, force: false);
        };
        state.Timer.Tick += state.Tick;
        state.Timer.Start();
    }

    private static void QueuePersist(Window window, State state)
    {
        if (string.IsNullOrEmpty(state.Options.PersistKey))
        {
            return;
        }
        state.PersistTimer ??= new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(400) };
        state.PersistTimer.Stop();
        state.PersistTimer.Tick -= state.PersistTick;
        state.PersistTick = (_, _) =>
        {
            state.PersistTimer?.Stop();
            Persist(window, state);
        };
        state.PersistTimer.Tick += state.PersistTick;
        state.PersistTimer.Start();
    }

    private static bool TryRestore(Window window, State state)
    {
        var key = state.Options.PersistKey;
        if (string.IsNullOrEmpty(key) || state.Options.Mode != ShellFitMode.UserResizable)
        {
            return false;
        }
        var prefs = ShellPrefs.Load();
        if (!prefs.WindowSizes.TryGetValue(key, out var saved) || saved.Width < 160 || saved.Height < 120)
        {
            return false;
        }
        var pixelW = saved.Width;
        var pixelH = saved.Height;
        ClampToWorkArea(window, ref pixelW, ref pixelH);
        state.Applying = true;
        try
        {
            window.AppWindow.Resize(new Windows.Graphics.SizeInt32(pixelW, pixelH));
        }
        finally
        {
            state.Applying = false;
        }
        state.UserSized = true;
        return true;
    }

    private static void Persist(Window window, State state)
    {
        var key = state.Options.PersistKey;
        if (string.IsNullOrEmpty(key) || !state.UserSized)
        {
            return;
        }
        var size = window.AppWindow.Size;
        if (size.Width < 160 || size.Height < 120)
        {
            return;
        }
        var prefs = ShellPrefs.Load();
        prefs.WindowSizes[key] = new ShellPrefs.WindowSize { Width = size.Width, Height = size.Height };
        ShellPrefs.Save(prefs);
    }

    private static void Apply(Window window, State state, bool force)
    {
        if (window.Content is not FrameworkElement root || root.XamlRoot == null)
        {
            return;
        }
        if (!force && state.Options.Mode == ShellFitMode.UserResizable && state.UserSized)
        {
            return;
        }
        var scale = root.XamlRoot.RasterizationScale;
        if (scale <= 0)
        {
            scale = 1;
        }
        root.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        var desired = root.DesiredSize;
        if (desired.Width < 1 || desired.Height < 1)
        {
            return;
        }
        var pad = state.Options.PadDip;
        var contentW = Math.Clamp(desired.Width + pad, state.Options.MinWidthDip, state.Options.MaxWidthDip);
        // Wrapped text measures as one line at unlimited width, so the height is taken again at the width the window will have.
        root.Measure(new Size(Math.Max(1, contentW - pad), double.PositiveInfinity));
        desired = root.DesiredSize;
        var contentH = Math.Clamp(desired.Height + pad, state.Options.MinHeightDip, state.Options.MaxHeightDip);
        FrameOverhead(window, scale, out var frameW, out var frameH);
        var pixelW = (int)Math.Ceiling(contentW * scale) + frameW;
        var pixelH = (int)Math.Ceiling(contentH * scale) + frameH;
        ClampToWorkArea(window, ref pixelW, ref pixelH);
        var current = window.AppWindow.Size;
        if (!force
            && Math.Abs(current.Width - pixelW) < 2
            && Math.Abs(current.Height - pixelH) < 2)
        {
            return;
        }
        if (!force
            && state.LastPixelW == pixelW
            && state.LastPixelH == pixelH)
        {
            return;
        }
        state.Applying = true;
        try
        {
            window.AppWindow.Resize(new Windows.Graphics.SizeInt32(pixelW, pixelH));
            state.LastPixelW = pixelW;
            state.LastPixelH = pixelH;
        }
        finally
        {
            state.Applying = false;
        }
    }

    private static void FrameOverhead(Window window, double scale, out int frameW, out int frameH)
    {
        frameW = (int)Math.Ceiling(16 * scale);
        frameH = (int)Math.Ceiling(40 * scale);
        var hwnd = WindowNative.GetWindowHandle(window);
        if (hwnd == IntPtr.Zero)
        {
            return;
        }
        if (!Native.GetWindowRect(hwnd, out var outer) || !Native.GetClientRect(hwnd, out var client))
        {
            return;
        }
        var outerW = outer.Right - outer.Left;
        var outerH = outer.Bottom - outer.Top;
        var clientW = client.Right - client.Left;
        var clientH = client.Bottom - client.Top;
        if (outerW > clientW && outerH > clientH && clientW > 0 && clientH > 0)
        {
            frameW = outerW - clientW;
            frameH = outerH - clientH;
        }
    }

    private static void ClampToWorkArea(Window window, ref int pixelW, ref int pixelH)
    {
        try
        {
            var area = DisplayArea.GetFromWindowId(window.AppWindow.Id, DisplayAreaFallback.Nearest);
            var work = area.WorkArea;
            var maxW = Math.Max(200, work.Width - 24);
            var maxH = Math.Max(160, work.Height - 24);
            pixelW = Math.Min(pixelW, maxW);
            pixelH = Math.Min(pixelH, maxH);
        }
        catch (Exception)
        {
        }
    }

    internal sealed class Options
    {
        public ShellFitMode Mode { get; init; } = ShellFitMode.Content;
        public double MinWidthDip { get; init; } = 280;
        public double MinHeightDip { get; init; } = 120;
        public double MaxWidthDip { get; init; } = 900;
        public double MaxHeightDip { get; init; } = 720;
        /// <summary>Outer chrome around measured content. Keep small — dialogs already use <see cref="Density.DialogMargin"/>.</summary>
        public double PadDip { get; init; } = 6;
        public string? PersistKey { get; init; }
    }

    private sealed class State
    {
        public Options Options { get; set; } = new();
        public bool UserSized;
        public bool Applying;
        public int LastPixelW;
        public int LastPixelH;
        public DispatcherTimer? Timer;
        public DispatcherTimer? PersistTimer;
        public EventHandler<object>? Tick;
        public EventHandler<object>? PersistTick;
        public TypedEventHandler<AppWindow, AppWindowChangedEventArgs>? OnAppChanged;
    }
}
