using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;
using DockSample.Settings;

namespace DockSample.Services;

/// <summary>アプリの設定（AppSettings）の読み込みと保存</summary>
public interface ISettingsService
{
    /// <summary>現在の設定。各 ViewModel はこれを読み書きする</summary>
    AppSettings Current { get; }

    /// <summary>settings.json に保存する</summary>
    void Save();
}

/// <summary>
/// %LocalAppData%\DockSample\settings.json に JSON で保存する。
/// （ファイルを消すと初期値に戻る）
/// </summary>
public class JsonSettingsService : ISettingsService
{
    private static readonly string FilePath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "DockSample", "settings.json");

    private static readonly JsonSerializerOptions Options = new()
    {
        WriteIndented = true,                            // 人が読める形で保存
        Converters = { new JsonStringEnumConverter() },  // enum を数字ではなく名前で保存
    };

    private readonly ILogService _log;

    public JsonSettingsService(ILogService log)
    {
        _log = log;
        Current = Load();
    }

    public AppSettings Current { get; }

    private AppSettings Load()
    {
        try
        {
            if (!File.Exists(FilePath)) return new AppSettings();
            var settings = JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(FilePath), Options);
            _log.Write("設定を読み込みました");
            return settings ?? new AppSettings();
        }
        catch (Exception ex)
        {
            // 壊れていたら初期値で起動する（ファイルは次の保存で上書きされる）
            _log.Write($"設定の読み込みに失敗したため初期値を使います: {ex.Message}");
            return new AppSettings();
        }
    }

    public void Save()
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);
            File.WriteAllText(FilePath, JsonSerializer.Serialize(Current, Options));
        }
        catch (Exception ex)
        {
            _log.Write($"設定の保存に失敗: {ex.Message}");
        }
    }
}
