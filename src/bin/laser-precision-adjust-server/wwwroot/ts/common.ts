$.noty.defaults.theme = "bootstrapTheme"

function noty_error(text: string, timeot: number = 5000) {
    noty({
        type: "error",
        text: "<i class='fa fa-times'></i> Ошибка: " + text,
        timeout: timeot
    });
}

function noty_success(text: string = "Операция выполнена успешно.", timeout: number = 3000) {
    noty({
        type: "success",
        text: "<i class='fa fa-check'></i> " + text,
        timeout: timeout
    });
}
