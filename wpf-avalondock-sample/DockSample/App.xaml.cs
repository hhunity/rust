using System.Windows;
using CommunityToolkit.Mvvm.Messaging;
using DockSample.Docking;
using DockSample.Services;
using DockSample.ViewModels;
using Microsoft.Extensions.DependencyInjection;

namespace DockSample;

public partial class App : Application
{
    private ServiceProvider? _provider;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        var services = new ServiceCollection();

        // Services
        services.AddSingleton<ILogService, LogService>();
        services.AddSingleton<IMessenger>(WeakReferenceMessenger.Default);
        services.AddSingleton<DockLayoutStore>();

        // ドッキングペイン。PaneViewModel として登録した順にトグルボタンが並ぶ。
        // ペインを増やすときはここに1行足して、App.xaml に DataTemplate を足すだけ。
        // 第2引数 = タブ/フローティングのタイトル、buttonText = 上部ボタンの表示（省略時はタイトルと同じ）
        services.AddSingleton<PaneViewModel>(_ => new DocumentViewModel("Document_1", "メモ.txt", buttonText: "メモ"));
        services.AddSingleton<PaneViewModel>(_ => new DocumentViewModel("Document_2", "ドキュメント2"));
        services.AddSingleton<PaneViewModel, ExplorerViewModel>();
        services.AddSingleton<PaneViewModel, PropertiesViewModel>();
        services.AddSingleton<PaneViewModel, OutputViewModel>();

        // Main
        services.AddSingleton<MainViewModel>();
        services.AddSingleton<MainWindow>();

        _provider = services.BuildServiceProvider();
        _provider.GetRequiredService<MainWindow>().Show();
    }

    protected override void OnExit(ExitEventArgs e)
    {
        _provider?.Dispose();
        base.OnExit(e);
    }
}
