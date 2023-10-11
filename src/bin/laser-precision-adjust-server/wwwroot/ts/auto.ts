// on page loaded jquery
$(() => {
    // https://getbootstrap.com/docs/4.0/components/tooltips/
    $('[data-toggle="tooltip"]').tooltip()

    $('.camera-ctrl').on('click', (ev) => {
        var action: string;
        if (ev.target.tagName === 'path') {
            action = $(ev.target).parent().parent().attr('ctrl-request');
        } else if (ev.target.tagName === 'svg') {
            action = $(ev.target).parent().attr('ctrl-request');
        } else {
            action = $(ev.target).attr('ctrl-request');
        }
        $.ajax({
            url: '/control/camera',
            method: 'POST',
            data: JSON.stringify({ CameraAction: action }),
            contentType: 'application/json',
            success: (data) => {
                if (!data.success) {
                    noty_error('Ошибка: ' + data.error);
                }
            }
        })
    });

    $('#adj-all-ctrl-btn').on('click', (ev) => {
        alert('Start!');
    });

    // report
    $('#gen-report').on('click', (ev) => {
        let report_id = prompt('Введите номер партии:');
        gen_report(report_id);
    });
});
