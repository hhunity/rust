using AvalonDock.Layout;
using DockSample.ViewModels;

namespace DockSample.Docking;

public class LayoutInitializer : ILayoutUpdateStrategy
{
    public bool BeforeInsertAnchorable(LayoutRoot layout, LayoutAnchorable anchorableToShow,
                                       ILayoutContainer destinationContainer)
    {
        // 一度隠したペインを再表示するときは、AvalonDock が覚えている元の位置に戻す
        if (destinationContainer is { Root: not null }) return false;
        if (((ILayoutPreviousContainer)anchorableToShow).PreviousContainer is { Root: not null }) return false;

        if (anchorableToShow.Content is not PaneViewModel pane) return false;

        // 初めて表示するときだけ、PreferredLocation に従って配置する
        if (pane.PreferredLocation == DockLocation.Document)
        {
            // ドキュメントは中央のドキュメント領域にタブとして入れる
            var documentPane = layout.Descendents().OfType<LayoutDocumentPane>().FirstOrDefault();
            if (documentPane == null) return false;
            documentPane.Children.Add(anchorableToShow);
            return true;
        }

        var (paneName, strategy) = pane.PreferredLocation switch
        {
            DockLocation.Left  => ("LeftPane",   AnchorableShowStrategy.Left),
            DockLocation.Right => ("RightPane",  AnchorableShowStrategy.Right),
            _                  => ("BottomPane", AnchorableShowStrategy.Bottom),
        };

        var anchorablePane = layout.Descendents()
                                   .OfType<LayoutAnchorablePane>()
                                   .FirstOrDefault(p => p.Name == paneName);

        if (anchorablePane != null)
        {
            anchorablePane.Children.Add(anchorableToShow);
        }
        else
        {
            // 空になったペインは AvalonDock に自動削除されることがあるので、その場合は端に新規作成
            anchorableToShow.AddToLayout(layout.Manager, strategy);
            if (anchorableToShow.Parent is LayoutAnchorablePane created)
                created.Name = paneName;
        }
        return true;
    }

    public void AfterInsertAnchorable(LayoutRoot layout, LayoutAnchorable anchorableShown) { }

    public bool BeforeInsertDocument(LayoutRoot layout, LayoutDocument anchorableToShow,
                                     ILayoutContainer destinationContainer) => false;

    public void AfterInsertDocument(LayoutRoot layout, LayoutDocument anchorableShown) { }
}
