# Управление установкой точной настройки кварцевых резонаторов лазером

## Based on
* [laser-setup-interface](https://github.com/ololoshka2871/Laser-setup-interface)
* [kosa-interface](https://github.com/ololoshka2871/kosa-interface)

# Зависимости
* [LibMan](https://learn.microsoft.com/ru-ru/aspnet/core/client-side/libman/libman-cli?view=aspnetcore-7.0) - менеджер зависимостей клиентских библиотек
    `dotnet tool install -g Microsoft.Web.LibraryManager.Cli`

# Настройки
- "DataLogFile": шаблон см [здесь](https://docs.rs/chrono/latest/chrono/struct.DateTime.html#method.format)

## Заметки
1. Установить число резов так, чтобы оно было как можно ближе кратно физическому разрешению сканатора!
2. Скорость реза (F) влияет на нагрев но практически не влияет на частоту реза. С другой стороны, если будет перебор не будет работать поиск края