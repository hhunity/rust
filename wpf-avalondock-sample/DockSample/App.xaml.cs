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

        // ツールウィンドウ（1つだけ存在する → Singleton）
        services.AddSingleton<ExplorerViewModel>();
        services.AddSingleton<PropertiesViewModel>();
        services.AddSingleton<OutputViewModel>();

        // ドキュメント（毎回新しく作る → Transient + ファクトリ）
        services.AddTransient<DocumentViewModel>();
        services.AddSingleton<Func<DocumentViewModel>>(sp => () => sp.GetRequiredService<DocumentViewModel>());

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
