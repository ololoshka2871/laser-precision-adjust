
interface ISearchingEdge {
    ch: number,
    step: number,
}

interface IProgressStatus {
    Idle?: Object,
    SearchingEdge?: ISearchingEdge,
    Adjusting?: Object,
    Done?: Object,
    Error?: string,
}

interface IRezInfo {
    id: number,
    current_step: number,
    current_freq: number,
    state: string,
}

interface IProgressReport {
    status: IProgressStatus,
    measure_channel_id?: number,
    burn_channel_id?: number,
    rezonator_info: Array<IRezInfo>,
}

interface IAutoAdjustStatusReport {
    progress_string: string,
    report: IProgressReport,
    reset_marker: boolean,
}

//-----------------------------------------------------------------------------

var updater: oboe.Oboe = null;

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
            data: JSON.stringify({}),
            contentType: 'application/json',
            success: (data) => {
                if (!data.success) {
                    noty_error('Ошибка: ' + data.error);
                } else if (data.message == 'Автонастройка отменена.') {
                    reset_gui();
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

    start_autoadjust_updater();
});

function update_autoadjust(report: IProgressReport, progress_string: string) {
    $('#adjust-step').text(progress_string);

    const set_btn_state = (start: boolean) => {
        const button = $('#adj-all-ctrl-btn');
        if (start && !button.hasClass('btn-warning')) {
            button.removeClass('btn-danger').addClass('btn-warning').text('Стоп');
        } else if (!start && !button.hasClass('btn-danger')) {
            button.removeClass('btn-warning').addClass('btn-danger').text('Начать');
        }
    };

    if (report.status.Error != undefined) {
        noty_error(report.status.Error);
        set_btn_state(true);
    } else if (report.status.Done != undefined) {
        noty_success("Настройка завершена!");
        set_btn_state(true);
    } else {
        set_btn_state(report.status.Idle == undefined);

        const measure_class = 'table-success';
        const burn_class = 'border border-3 border-danger';
        const sel_header = (ch: number) => 'th[position="' + (ch + 1).toString() + '"]'

        function update_text_if_changed(selector: string, value: string) {
            const item = $(selector);
            if (item.text() != value) {
                item.text(value);
            }
        }

        if (report.burn_channel_id != undefined) {
            const td = $(sel_header(report.burn_channel_id));
            if (!td.hasClass(burn_class)) {
                const siblings = td.parent().siblings()
                siblings.children('th').removeClass(burn_class);
                siblings.children('td').removeClass(burn_class);
                td.addClass(burn_class).siblings().addClass(burn_class);
            }
        } else {
            $('th').removeClass(burn_class);
            $('td').removeClass(burn_class);
        }

        if (report.measure_channel_id != undefined) {
            const tr = $(sel_header(report.measure_channel_id)).parent();
            if (!tr.hasClass(measure_class)) {
                tr.addClass(measure_class).siblings().removeClass(measure_class);
            }
        } else {
            $('tr').removeClass(measure_class);
        }

        for (const rez of report.rezonator_info) {
            const posid: string = '[position="' + (rez.id + 1).toString() + '"]';
            update_text_if_changed('td.position-display' + posid, rez.current_step.toString());
            update_text_if_changed('td.frequency-display' + posid, round_to_2_digits(rez.current_freq));
            update_text_if_changed('td.status-display' + posid, rez.state);
        }
    }
}

function start_autoadjust_updater() {
    updater = oboe('/auto_status')
        .done((report: IAutoAdjustStatusReport | IControlResult) => {
            if ((<IControlResult>report).success == undefined) {
                const rep = report as IAutoAdjustStatusReport;
                update_autoadjust(rep.report, rep.progress_string);
                if (rep.reset_marker) {
                    setTimeout(() => start_autoadjust_updater(), 0)
                }
            } else {
                reset_gui();
            }
        });
}

function reset_gui() {
    updater.abort();
    update_autoadjust({
        status: { Idle: Object() },
        rezonator_info: []
    }, "Ожидание");
}