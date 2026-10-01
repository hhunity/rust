using System.IO;
using AvalonDock;
using AvalonDock.Layout;
using AvalonDock.Serializer.Xml;   // v5: 旧 AvalonDock.Layout.Serialization
using DockSample.Services;
using DockSample.ViewModels;

namespace DockSample.Docking;

/// <summary>
/// ドッキング配置をファイルに保存/復元する。
/// 保存先: %LocalAppData%\DockSample\layout.xml（消すと初期配置に戻る）
/// </summary>
public class DockLayoutStore
{
    private static readonly string FilePath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "DockSample", "layout.xml");

    private readonly ILogService _log;

    public DockLayoutStore(ILogService log) => _log = log;

    public void Load(DockingManager manager, IEnumerable<PaneViewModel> panes)
    {
        if (!File.Exists(FilePath)) return;

        var serializer = new XmlLayoutSerializer(manager);
        serializer.LayoutSerializationCallback += (_, e) =>
        {
            // ContentId から ViewModel を探して結び付け直す
            var pane = panes.FirstOrDefault(p => p.ContentId == e.Model.ContentId);
            if (pane == null)
            {
                // もう存在しないペインは捨てる
                e.Cancel = true;
                return;
            }

            e.Content = pane;
            // 表示/非表示の状態も ViewModel に反映（ToggleButton の状態と一致させる）
            if (e.Model is LayoutAnchorable anchorable)
                pane.IsVisible = !anchorable.IsHidden;
        };

        try
        {
            serializer.Deserialize(FilePath);
            _log.Write("前回のレイアウトを復元しました");
        }
        catch (Exception ex)
        {
            _log.Write($"レイアウトの復元に失敗: {ex.Message}");
        }
    }

    public void Save(DockingManager manager)
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);
            new XmlLayoutSerializer(manager).Serialize(FilePath);
        }
        catch (Exception ex)
        {
            _log.Write($"レイアウトの保存に失敗: {ex.Message}");
        }
    }
}
