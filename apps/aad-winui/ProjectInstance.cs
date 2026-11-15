using System.IO.Pipes;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Text.Json;

namespace Aad.WinUi;

/// <summary>
/// One MynaCAD per project (docs/ops-first-workflow.md, section 2.1 and 2.3): a named mutex, an activation pipe for the suite-internal requests and
/// the same-PC lease <c>%LOCALAPPDATA%\O-Studio\leases\&lt;hash&gt;.cad.lock</c>, which is held open with no sharing for as long as the process lives
/// (so a crash leaves no stale lease). The hash is the first 16 hex digits of the SHA-256 of the lower-case full path.
/// </summary>
internal sealed class ProjectInstance : IDisposable
{
    private const int MaxLine = 64 * 1024;

    private readonly Mutex? _mutex;
    private readonly FileStream? _lease;
    private CancellationTokenSource? _stop;

    private ProjectInstance(string project, string hash, Mutex? mutex, FileStream? lease, bool acquired)
    {
        Project = project;
        Hash = hash;
        _mutex = mutex;
        _lease = lease;
        Acquired = acquired;
    }

    /// <summary>The instance of this process, set when it owns a project.</summary>
    public static ProjectInstance? Current { get; private set; }

    public string Project { get; }

    public string Hash { get; }

    /// <summary>False when another MynaCAD already has this project: this process is only a client of it.</summary>
    public bool Acquired { get; }

    public static string HashOf(string project)
    {
        var full = Path.GetFullPath(project).ToLowerInvariant();
        var bytes = SHA256.HashData(Encoding.UTF8.GetBytes(full));
        return Convert.ToHexString(bytes, 0, 8).ToLowerInvariant();
    }

    public static string MutexName(string hash) => @"Local\MynaCAD.Project." + hash;

    public static string PipeName(string hash) => "MynaCAD." + hash;

    /// <summary>The lease folder. <c>OSTUDIO_LEASE_DIR</c> moves only it, <c>OSTUDIO_LOCAL_DIR</c> moves the whole O-Studio local folder (test and hand-run hooks, as MynaOps honours them).</summary>
    public static string LeaseFolder()
    {
        var lease = Environment.GetEnvironmentVariable("OSTUDIO_LEASE_DIR");
        if (!string.IsNullOrWhiteSpace(lease))
        {
            return lease.Trim();
        }
        var local = Environment.GetEnvironmentVariable("OSTUDIO_LOCAL_DIR");
        return !string.IsNullOrWhiteSpace(local)
            ? Path.Combine(local.Trim(), "leases")
            : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "O-Studio", "leases");
    }

    /// <summary>Take the project: the mutex first, then the lease. A second start for the same project does not get it.</summary>
    public static ProjectInstance TryAcquire(string project)
    {
        var hash = HashOf(project);
        Mutex? mutex = null;
        try
        {
            mutex = new Mutex(true, MutexName(hash), out var created);
            if (!created)
            {
                mutex.Dispose();
                return new ProjectInstance(project, hash, null, null, acquired: false);
            }
        }
        catch (Exception e) when (e is UnauthorizedAccessException or IOException or WaitHandleCannotBeOpenedException)
        {
            return new ProjectInstance(project, hash, null, null, acquired: false);
        }
        FileStream? lease = null;
        try
        {
            Directory.CreateDirectory(LeaseFolder());
            lease = new FileStream(Path.Combine(LeaseFolder(), hash + ".cad.lock"), FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None);
            var body = JsonSerializer.Serialize(new Dictionary<string, object>
            {
                ["app"] = "mynacad",
                ["pid"] = Environment.ProcessId,
                ["started_utc"] = DateTime.UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", System.Globalization.CultureInfo.InvariantCulture),
                ["project"] = project,
            });
            lease.SetLength(0);
            lease.Write(Encoding.UTF8.GetBytes(body));
            lease.Flush();
        }
        catch (IOException)
        {
            // Something else holds the lease of this project: it is not ours to take.
            lease?.Dispose();
            mutex.ReleaseMutex();
            mutex.Dispose();
            return new ProjectInstance(project, hash, null, null, acquired: false);
        }
        catch (UnauthorizedAccessException)
        {
            // No lease folder to write (a locked-down profile): the mutex alone still keeps one MynaCAD per project.
            lease = null;
        }
        var instance = new ProjectInstance(project, hash, mutex, lease, acquired: true);
        Current = instance;
        return instance;
    }

    /// <summary>Send one request line to the MynaCAD that has the project; returns its reply line, or null when it cannot be reached.</summary>
    public static string? Forward(string project, string requestLine, int timeoutMs = 4000)
    {
        try
        {
            using var client = new NamedPipeClientStream(".", PipeName(HashOf(project)), PipeDirection.InOut, PipeOptions.None);
            client.Connect(timeoutMs);
            var bytes = Encoding.UTF8.GetBytes(requestLine.Replace("\r", "").Replace("\n", "") + "\n");
            client.Write(bytes);
            client.Flush();
            return ReadLine(client, timeoutMs);
        }
        catch (Exception e) when (e is IOException or TimeoutException or UnauthorizedAccessException or InvalidOperationException)
        {
            return null;
        }
    }

    /// <summary>The activation pipe: every line is a JSON request, every reply a JSON line. <paramref name="handle"/> runs on a pool thread.</summary>
    public void StartServer(Func<JsonElement, Task<object>> handle)
    {
        if (!Acquired || _stop != null)
        {
            return;
        }
        _stop = new CancellationTokenSource();
        var token = _stop.Token;
        _ = Task.Run(async () =>
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    await using var server = Create();
                    await server.WaitForConnectionAsync(token);
                    var line = ReadLine(server, 5000);
                    object reply;
                    try
                    {
                        using var doc = JsonDocument.Parse(line ?? "");
                        reply = doc.RootElement.ValueKind == JsonValueKind.Object
                            ? await handle(doc.RootElement.Clone())
                            : new { ok = false, error = "a request is a JSON object" };
                    }
                    catch (JsonException)
                    {
                        reply = new { ok = false, error = "not a JSON request" };
                    }
                    var bytes = Encoding.UTF8.GetBytes(JsonSerializer.Serialize(reply) + "\n");
                    await server.WriteAsync(bytes, token);
                    await server.FlushAsync(token);
                    try
                    {
                        server.WaitForPipeDrain();
                    }
                    catch (IOException)
                    {
                    }
                }
                catch (OperationCanceledException)
                {
                    return;
                }
                catch (Exception e) when (e is IOException or UnauthorizedAccessException or InvalidOperationException)
                {
                    // A client that went away: serve the next one.
                    await Task.Delay(100, CancellationToken.None);
                }
            }
        });
    }

    private NamedPipeServerStream Create()
    {
        var security = new PipeSecurity();
        var user = WindowsIdentity.GetCurrent().User;
        if (user != null)
        {
            // This user only.
            security.AddAccessRule(new PipeAccessRule(user, PipeAccessRights.ReadWrite | PipeAccessRights.CreateNewInstance, System.Security.AccessControl.AccessControlType.Allow));
        }
        return NamedPipeServerStreamAcl.Create(PipeName(Hash), PipeDirection.InOut, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous, 4096, 4096, security);
    }

    /// <summary>One line, at most <see cref="MaxLine"/> bytes (a hostile client cannot make the process allocate more).</summary>
    private static string? ReadLine(Stream stream, int timeoutMs)
    {
        var buffer = new List<byte>();
        var one = new byte[1];
        using var cts = new CancellationTokenSource(timeoutMs);
        try
        {
            while (buffer.Count < MaxLine)
            {
                var n = stream.ReadAsync(one, 0, 1, cts.Token).GetAwaiter().GetResult();
                if (n == 0 || one[0] == (byte)'\n')
                {
                    break;
                }
                buffer.Add(one[0]);
            }
        }
        catch (OperationCanceledException)
        {
        }
        return buffer.Count == 0 ? null : Encoding.UTF8.GetString(buffer.ToArray());
    }

    public void Dispose()
    {
        _stop?.Cancel();
        _lease?.Dispose();
        if (_mutex != null)
        {
            try
            {
                _mutex.ReleaseMutex();
            }
            catch (ApplicationException)
            {
            }
            _mutex.Dispose();
        }
        if (ReferenceEquals(Current, this))
        {
            Current = null;
        }
    }
}
