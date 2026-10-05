using System.Windows;
using DockSample.Docking;
using DockSample.Services;
using DockSample.ViewModels;

namespace DockSample;

public partial class MainWindow : Window
{
    // コンストラクタインジェクション
    public MainWindow(MainViewModel viewModel, DockLayoutStore layoutStore, ISettingsService settings)
    {
        InitializeComponent();
        DataContext = viewModel;

        // レイアウトの復元・保存は DockingManager(View) が必要なので View 側で行う
        Loaded += (_, _) => layoutStore.Load(DockManager, viewModel.Panes);

        Closing += (_, e) =>
        {
            // 未保存の設定があれば確認する（「いいえ」なら保存せずに終了 → 次回は前回保存した値で起動）
            if (settings.IsDirty)
            {
                var result = MessageBox.Show(this,
                    "設定に保存していない変更があります。保存しますか？",
                    "DockSample", MessageBoxButton.YesNoCancel, MessageBoxImage.Question);

                if (result == MessageBoxResult.Cancel) { e.Cancel = true; return; }
                if (result == MessageBoxResult.Yes) settings.Save();
            }

            layoutStore.Save(DockManager);
        };
    }
}
