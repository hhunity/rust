using System.Collections.ObjectModel;

namespace DockSample.Services;

public interface ILogService
{
    ObservableCollection<string> Messages { get; }
    void Write(string message);
}

public class LogService : ILogService
{
    public ObservableCollection<string> Messages { get; } = new();

    public void Write(string message) =>
        Messages.Add($"[{DateTime.Now:HH:mm:ss}] {message}");
}
