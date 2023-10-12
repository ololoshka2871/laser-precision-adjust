
interface IAutoAdjustStatusReport {
    active: boolean,
    status: string,
    reset_marker: boolean,
}

//-----------------------------------------------------------------------------

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
        $.ajax({
            url: '/control/adjust-all',
            method: 'POST',
            data: JSON.stringify({ }),
            contentType: 'application/json',
            success: (data) => {
                if (!data.success) {
                    noty_error('Ошибка: ' + data.error);
                } else {
                    start_autoadjust_updater();
                }
            }
        })
        ev.preventDefault();
    });

    // report
    $('#gen-report').on('click', (ev) => {
        let report_id = prompt('Введите номер партии:');
        gen_report(report_id);
    });
});

function update_autoadjust(state: IAutoAdjustStatusReport) {
    console.log(state);
}

function start_autoadjust_updater() {
    oboe('/auto_status')
        .done((state: IAutoAdjustStatusReport) => {
            update_autoadjust(state);
            if (state.reset_marker) {
                setTimeout(() => start_autoadjust_updater(), 0)
            }
        })
}