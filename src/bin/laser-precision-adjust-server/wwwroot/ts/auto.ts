
type Done = "Done";
type Adjusting = "Adjusting";
type Idle = "Idle";

interface ISearchingEdge {
    ch: number,
    step: number,
}

interface IProgressStatus {
    SearchingEdge?: ISearchingEdge,
    Error?: string,
}

interface IRezInfo {
    id: number,
    current_step: number,
    initial_freq: number,
    current_freq: number,
    state: string,
}

interface IProgressReport {
    status: IProgressStatus | Idle | Done | Adjusting,
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
                } else if (action == 'close') {
                    update_vac_button();
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
        });
        ev.preventDefault();
    });

    // report
    $('#gen-report').on('click', (ev) => {
        let report_id = prompt('Введите номер партии:');
        gen_report2(report_id);
        ev.preventDefault();
    });

    $('#freq-target').on('input', (ev) => patch_value(ev.target as HTMLInputElement, 'TargetFreq'));
    $('#freq_adj').on('input', (ev) => patch_value(ev.target as HTMLInputElement, 'WorkOffsetHz'));

    update_vac_button();

    start_autoadjust_updater();
});

function update_autoadjust(report: IProgressReport, progress_string: string) {
    const measure_class = 'sensor';
    const burn_class = 'burner';

    $('#adjust-step').text(progress_string);

    function set_btn_state(start: boolean) {
        const button = $('#adj-all-ctrl-btn');
        if (start && !button.hasClass('btn-warning')) {
            button.removeClass('btn-danger').addClass('btn-warning').text('Стоп');
        } else if (!start && !button.hasClass('btn-danger')) {
            button.removeClass('btn-warning').addClass('btn-danger').text('Начать');
        }
    }

    function reset_laser_pos() {
        $('tr').removeClass(burn_class);
    }

    function reset_freqmeter_pos() {
        $('tr').removeClass(measure_class);
    }

    if ((<IProgressStatus>report.status).Error != undefined) {
        noty_error((report.status as IProgressStatus).Error);
        set_btn_state(true);
        reset_laser_pos();
        reset_freqmeter_pos();
        return;
    } else if (report.status == "Done") {
        noty_success("Настройка завершена!");
        set_btn_state(false);
        reset_laser_pos();
        reset_freqmeter_pos();
    } else {
        set_btn_state(report.status !== "Idle");
    }

    const sel_header = (ch: number) => 'th[rez-pos="' + (ch + 1).toString() + '"]'

    function update_text_if_changed(selector: string, value: string) {
        const item = $(selector);
        if (item.text() != value) {
            item.text(value);
        }
    }

    if (report.burn_channel_id !== undefined) {
        const tr = $(sel_header(report.burn_channel_id)).parent();
        if (!tr.hasClass(burn_class)) {
            tr.addClass(burn_class).siblings().removeClass(burn_class);
        }
    } else {
        reset_laser_pos();
    }

    if (report.measure_channel_id !== undefined) {
        const tr = $(sel_header(report.measure_channel_id)).parent();
        if (!tr.hasClass(measure_class)) {
            tr.addClass(measure_class).siblings().removeClass(measure_class);
        }
    } else {
        reset_freqmeter_pos();
    }

    console.log("Measure: [" + report.measure_channel_id + "] Burn: [" + report.burn_channel_id + "]");

    for (const rez of report.rezonator_info) {
        const posid: string = '[rez-pos="' + (rez.id + 1).toString() + '"]';
        update_text_if_changed('td.position-display' + posid, rez.current_step.toString());
        update_text_if_changed('td.start-freq-display' + posid, round_to_2_digits(rez.initial_freq));
        update_text_if_changed('td.status-display' + posid, rez.state);

        draw_progress(rez.initial_freq, rez.current_freq, $('td.frequency-display' + posid));
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
        status: "Idle",
        rezonator_info: []
    }, "Ожидание");
}

function draw_progress(inital_freq: number, current_freq: number, cell: JQuery<HTMLElement>) {
    const cache = cell.children();
    cell.text(round_to_2_digits(current_freq)).append(cache);

    if (inital_freq !== 0.0 || current_freq === 0.0) {
        const $freq_target = $('#freq-target');
        const target = parseFloat($freq_target.val() as string);
        const precision_hz = parseFloat($freq_target.attr('precision_hz'));
        const min_f = target - precision_hz;
        const max_f = target + precision_hz;

        // Start --- min_f target max_f --- f
        // | -------- | ----- * ----| ----- |
        const progress_marker = cache.filter('.progress-marker');
        if (current_freq < inital_freq || Math.abs(current_freq - inital_freq) < 0.2) {
            inital_freq = Math.min(inital_freq - 2 * precision_hz, min_f);
        }

        var full_percent: number;
        if (max_f > current_freq) {
            // правый упор - маркер max_f
            full_percent = (max_f - inital_freq) / 100;
            cache.filter('.max-marker').css('left', '99%');
            progress_marker.css('width', ((current_freq - inital_freq) / full_percent).toString() + '%');
            if (current_freq > min_f) {
                progress_marker.css('background-color', 'rgba(33, 204, 70, 0.75)');
            } else {
                progress_marker.css('background-color', 'rgba(219, 223, 104, 0.75)');
            }
        } else {
            // правый упор - текущая частота
            full_percent = (current_freq - inital_freq) / 100;
            cache.filter('.max-marker').css('left', ((max_f - inital_freq) / full_percent - 1).toString() + '%');
            progress_marker.css('width', '100%');
            progress_marker.css('background-color', 'rgba(233, 86, 86, 0.75)');
        }
        cache.filter('.target-marker').css('left', ((target - inital_freq) / full_percent - 1).toString() + '%');
        cache.filter('.min-marker').css('left', ((min_f - inital_freq) / full_percent).toString() + '%');
    }
}

function update_vac_button(secs: number = 20) {
    var $button = $('button[ctrl-request="vac"]');
    if (secs > 0) {
        $button
            .prop('disabled', 'disabled')
            .html('<i class="fas fa-spinner fa-pulse"></i> Ждите... (' + secs + ')');
        setTimeout(update_vac_button, 1000, secs - 1);
    } else {
        $button.prop('disabled', false)
            .prop('data-state', 'enabled')
            .html('<i class="fa fa-soap"></i> Вакуум');
    }
}

function gen_report2(report_id: string) {
    var link = document.createElement("a");
    // If you don't know the name or want to use
    // the webserver default set name = ''
    link.setAttribute('download', report_id);
    link.href = '/report2/' + report_id;
    document.body.appendChild(link);
    link.click();
    link.remove();
}