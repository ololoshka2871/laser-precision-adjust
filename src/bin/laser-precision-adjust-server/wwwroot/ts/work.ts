// define interface for jquery JQuery<HTMLElement> where method tooltip is defined
interface JQuery<TElement extends Element = HTMLElement> extends Iterable<TElement> {
    tooltip(options?: any): JQuery<TElement>;
}

// declare hotkeys as defined global function
declare function hotkeys(key: string, callback: (event: KeyboardEvent, handler: any) => void): void;

// ---------------------------------------------------------------------------------------------

interface IPrediction {
    start_offset: number,
    minimal: number,
    maximal: number,
    median: number,
}

interface IState {
    TimesTamp: number
    SelectedChannel: number
    CurrentFreq: number
    TargetFreq: number
    WorkOffsetHz: number
    CurrentStep: number
    InitialFreq: number
    Points: [number, number][] // [timestamp, freq]
    Prediction?: IPrediction,
    CloseTimestamp?: number,
    Aproximations: Array<Array<[number, number]>>,
    IsAutoAdjustBusy: boolean,
    StatusCode: string,
    RestartMarker: boolean,
}

interface IControlResult {
    success: boolean,
    error?: string,
    message?: string,
}

// ---------------------------------------------------------------------------------------------

let present_noty: Noty = null;

// on page loaded jquery
$(() => {
    // https://www.chartjs.org/docs/2.9.4/getting-started/integration.html#content-security-policy
    Chart.platform.disableCSSInjection = true;

    // https://getbootstrap.com/docs/4.0/components/tooltips/
    $('[data-toggle="tooltip"]').tooltip()

    $('#rezonators').on('click', 'tbody tr', (ev) => {
        let channel_to_select: number;
        if (ev.target.tagName === 'TD' || ev.target.tagName === 'TH') {
            // <td> / <th>
            channel_to_select = parseInt($(ev.target)
                .parent()
                .attr('row-index')) - 1;
        } else if ((ev.target.tagName === 'CODE') || (ev.target.tagName === 'svg')) {
            // <td><code></td> / <td><svg></td>
            channel_to_select = parseInt($(ev.target)
                .parent()
                .parent()
                .attr('row-index')) - 1;
        } else if (ev.target.tagName === 'path') {
            // <td><svg><path></svg></td>
            channel_to_select = parseInt($(ev.target)
                .parent()
                .parent()
                .parent()
                .attr('row-index')) - 1;
        }

        $.ajax({
            url: '/control/select',
            method: 'POST',
            data: JSON.stringify({ Channel: channel_to_select }),
            contentType: 'application/json',
            success: (data) => {
                if (data.success) {
                    noty_success('Выбран резонатор №' + (channel_to_select + 1).toString());
                } else {
                    noty_error('Ошибка: ' + data.error);
                }
            }
        })
    });

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

    $('#move-to-btn').on('click', (ev) => { move_to(); ev.preventDefault(); });
    $('#burn-btn').on('click', (ev) => { burn(); ev.preventDefault(); });
    $('#scan-all-btn').on('click', (ev) => {
        oboe({
            url: '/control/scan-all',
            method: 'POST',
            body: {}
        }).done((state: IControlResult) => {
            if (state.success) {
                if (state.message === 'Finished' && present_noty !== null) {
                    present_noty.close();
                    present_noty = null;
                } else if (present_noty !== null) {
                    present_noty.setText("<i class='fas fa-spinner fa-pulse'></i> " + state.message);
                } else {
                    present_noty = noty({
                        type: "information",
                        text: "<i class='fas fa-spinner fa-pulse'></i> " + state.message,
                    });
                }
            } else {
                noty_error('Ошибка: ' + state.error);
            }
        });

        ev.preventDefault();
    });

    $('#adj-ctrl-btn').on('click', (ev) => {
        oboe({
            url: '/control/auto-adjust',
            method: 'POST',
            body: {}
        }).done((state: IControlResult) => {
            if (state.success) {
                console.log(state.message);

                if ((state.message.startsWith('Настройка завершена') || state.message.startsWith('Настройка отменена')) && present_noty !== null) {
                    present_noty.close();
                    present_noty = null;
                } else if (present_noty !== null) {
                    present_noty.setText("<i class='fas fa-spinner fa-pulse'></i> " + state.message);
                } else {
                    present_noty = noty({
                        type: "information",
                        text: "<i class='fas fa-spinner fa-pulse'></i> " + state.message,
                    });
                }
            } else {
                console.log(state.error);
                if (present_noty !== null) {
                    present_noty.close();
                    present_noty = null;
                }
                noty_error(state.error);
            }
        });

        ev.preventDefault();
    });

    $('#freq-target').on('input', (ev) => patch_value(ev.target as HTMLInputElement, 'TargetFreq'));
    $('#freq_adj').on('input', (ev) => patch_value(ev.target as HTMLInputElement, 'WorkOffsetHz'));

    const chart = new Chart(
        $('#adj-plot').get()[0] as HTMLCanvasElement,
        {
            type: 'line',
            data: {
                labels: [],
                datasets: [
                    { // 0
                        label: 'Upper Limit',
                        lineTension: 0,
                        pointRadius: 0,
                        fill: 'top',
                        backgroundColor: 'rgba(240, 81, 81, 0.5)',
                        borderColor: 'rgb(240, 81, 81)',
                    },
                    { // 1
                        label: 'Lower Limit',
                        lineTension: 0,
                        pointRadius: 0,
                        fill: 'bottom', // заполнить область до графика 1
                        backgroundColor: 'rgba(204, 167, 80, 0.5)',
                        borderColor: 'rgb(204, 167, 80)',
                    },
                    { // 2
                        label: 'Actual',
                        lineTension: 0,
                        pointRadius: 2.5,
                        fill: false,
                        showLine: false,
                        borderColor: 'rgba(75, 148, 204, 30)',
                    },
                    { // 3
                        label: 'Target',
                        lineTension: 0,
                        pointRadius: 0,
                        fill: false,
                        borderColor: 'rgb(8, 150, 38)',
                    },
                    { // 4
                        label: 'Median_predict',
                        lineTension: 0,
                        pointRadius: 0,
                        fill: false,
                        borderColor: 'rgb(25, 133, 29, 0.6)',
                    },
                    { // 5
                        label: 'Upper_predict',
                        lineTension: 0,
                        pointRadius: 0,
                        fill: 4,
                        borderColor: 'rgb(133, 21, 42, 0.6)',
                    },
                    { // 6
                        label: 'Lower_predict',
                        lineTension: 0,
                        pointRadius: 0,
                        fill: 4,
                        borderColor: 'rgb(133, 21, 42, 0.6)',
                    },
                    { // 7
                        label: 'aproximation',
                        lineTension: 0,
                        pointRadius: 0,
                        fill: false,
                        borderColor: 'green',
                    }
                ]
            },
            options: {
                animation: {
                    duration: 0
                },
                tooltips: {
                    enabled: false // <-- this option disables tooltips
                },
                responsive: true,
                aspectRatio: 1,
                legend: {
                    display: false,
                },
                scales: {
                    xAxes: [{
                        display: false, //this will remove all the x-axis grid lines
                        ticks: {
                            display: false //this will remove only the label
                        }
                    }],
                    yAxes: [{
                        type: 'linear',
                        position: 'left',
                    }]
                }
            }
        });

    start_updater(chart);

    // hotkeys
    hotkeys('space', (event, _handler) => {
        event.preventDefault();
        move_to();
    });

    hotkeys('enter', (event, _handler) => {
        event.preventDefault();
        burn();
    });

    hotkeys('left', (event, _handler) => {
        event.preventDefault();
        move_rel(1);
    });

    hotkeys('right', (event, _handler) => {
        event.preventDefault();
        move_rel(-1);
    });

    // report
    $('#gen-report').on('click', (ev) => {
        let report_id = prompt('Введите номер партии:');
        gen_report(report_id);
    });
});

function update_f_re_display(cfg): void {
    const value = round_to_2_digits(cfg.freq);
    $('#current-freq-display').text(value);

    const bg_class = value < cfg.min
        ? 'bg-warning'
        : (value > cfg.max ? 'bg-danger' : 'bg-success');

    const display_bg = $('#current-freq-display-bg');
    if (!display_bg.hasClass(bg_class)) {
        display_bg.removeClass('bg-success bg-danger bg-warning').addClass(bg_class);
    }
}

function update_rezonator_table(state: IState): void {
    const primary_class = 'bg-primary';
    const newly_selected = $('#rez-' + (state.SelectedChannel + 1).toString() + '-row');
    if (!newly_selected.hasClass(primary_class)) {
        newly_selected.addClass(primary_class).siblings().removeClass(primary_class);
    }
    newly_selected.children('[type=step]').children().text(state.CurrentStep);
    newly_selected.children('[type=freq-current]').children().text(round_to_2_digits(state.CurrentFreq));
    newly_selected.children('[type=freq-initial]').children().text(round_to_2_digits(state.InitialFreq));

    const icon_td = newly_selected.children('[type=freq-status-icon]');
    if (icon_td.prop('status') != state.StatusCode) {
        icon_td.prop('status', state.StatusCode);
        icon_td.removeClass();
        switch (state.StatusCode) {
            case "upper":
                icon_td.removeClass().addClass('text-danger');
                icon_td.html('<i class="fas fa-caret-square-up"></i>');
                break;
            case "ok":
                icon_td.removeClass().addClass('text-success');
                icon_td.html('<i class="fas fa-check"></i>');
                break;
            case "lower":
                icon_td.removeClass().addClass('text-warning');
                icon_td.html('<i class="fas fa-caret-square-down"></i>');
                break;
            case "lowerest":
                icon_td.removeClass().addClass('text-danger');
                icon_td.html('<i class="fa fa-caret-square-down"></i>');
                break;
            default:
                icon_td.removeClass().addClass('text-primary');
                icon_td.html('<i class="fa fa-minus"></i>');
                break;
        }
    }
}

function burn(): void {
    const autostep = $('#auto-offset-input').val();

    let autostep_val: Number;
    if (autostep === '') {
        autostep_val = 0;
    } else {
        autostep_val = parseInt(autostep as string);
        if (Number.isNaN(autostep_val)) {
            noty_error('Неверное значение автошага (не целое число)');
            return;
        }
    }

    $.ajax({
        url: '/control/burn',
        method: 'POST',
        data: JSON.stringify({ MoveOffset: autostep_val }),
        contentType: 'application/json',
        success: (data) => {
            if (!data.success) {
                noty_error('Ошибка: ' + data.error);
            }
        }
    });
}

function move_rel(offset: number): void {
    $.ajax({
        url: '/control/move',
        method: 'POST',
        data: JSON.stringify({ MoveOffset: offset }),
        contentType: 'application/json',
        success: (data) => {
            if (!data.success) {
                noty_error('Ошибка: ' + data.error);
            }
        }
    });
}

function move_to(): void {
    const target = $('#move-to-input').val();
    if (target === '' || Number.isNaN(parseFloat(target as string))) {
        return;
    } else {
        const target_val = parseFloat(target as string) || 0;
        $.ajax({
            url: '/control/move',
            method: 'POST',
            data: JSON.stringify({ TargetPosition: target_val }),
            contentType: 'application/json',
            success: (data) => {
                if (!data.success) {
                    noty_error('Ошибка: ' + data.error);
                }
            }
        })
    }
}

function update_camera_controls(close_timestamp: number | null, last_timestamp: number | undefined): void {
    // если close_timestamp === null, то камера открыта, нельзя включать ваккум
    // если last_timestamp - close_timestamp > 20 сек., камера закрыта, можно включать ваккум
    const close_time = 20
    const vacuum_btn = $('button[ctrl-request=vac]');

    const after_close_s = Math.round((last_timestamp - close_timestamp) / 1_000);
    if (close_timestamp === null) {
        if (vacuum_btn.prop('data-state') !== 'disabled') {
            vacuum_btn.prop('disabled', 'disabled')
                .prop('data-state', 'disabled')
                .html('<i class="fa fa-soap"></i> Вакуум');
        }
    } else if (after_close_s > close_time) {
        if (vacuum_btn.prop('data-state') !== 'enabled') {
            vacuum_btn.prop('disabled', false)
                .prop('data-state', 'enabled')
                .html('<i class="fa fa-soap"></i> Вакуум');
        }
    } else {
        const remaning = (close_time - after_close_s).toString();
        if (vacuum_btn.prop('data-state') !== 'w' + remaning) {
            vacuum_btn.prop('disabled', 'disabled')
                .prop('data-state', 'w' + remaning)
                .html('<i class="fas fa-spinner fa-pulse"></i> Ждите... (' + remaning + ')');
        }
    }
}

function update_autoadj_button(busy: boolean): void {
    const btn = $('#adj-ctrl-btn');
    if (busy && !btn.hasClass('btn-warning')) {
        // make cancel
        btn.removeClass('btn-danger').addClass('btn-warning').text('Стоп');
    } else if (!busy && !btn.hasClass('btn-danger')) {
        // make start
        btn.removeClass('btn-warning').addClass('btn-danger').text('Настроить');
    }
}

function update(chart: Chart, state: IState): void {
    // state - это весь JSON объект, который пришел с сервера
    const current_freq = state.CurrentFreq;

    const target = state.TargetFreq;
    const offset_hz = state.WorkOffsetHz;
    const upperLimit = target + offset_hz;
    const lowerLimit = target - offset_hz;

    // добавляем новые значения в график
    chart.data.labels = state.Points.map(p => p[0]);
    chart.data.datasets[0].data = Array<number>(state.Points.length).fill(upperLimit);
    chart.data.datasets[1].data = Array<number>(state.Points.length).fill(lowerLimit);
    // raw data
    chart.data.datasets[2].data = state.Points.map(p => p[1]);
    chart.data.datasets[3].data = Array<number>(state.Points.length).fill(target);

    var plot_max = upperLimit;
    var plot_min: number;
    {
        var data_not_nan = chart.data.datasets[2].data.filter((v?: number) => v !== null) as number[];
        var sl = data_not_nan.slice(0, Math.min(5, data_not_nan.length));
        const data_start_min_avg = sl.reduce((a, b) => a + b, 0) / sl.length;
        sl = data_not_nan.slice(data_not_nan.length - Math.min(5, data_not_nan.length), data_not_nan.length);
        const data_end_min_avg = sl.reduce((a, b) => a + b, 0) / sl.length;
        plot_min = Math.min(data_start_min_avg, data_end_min_avg, lowerLimit) - 1.0;
    }

    // prediction
    if (state.Prediction !== undefined && state.Points.length > 5) {
        const offset = state.Prediction.start_offset || 0;
        chart.data.datasets[4].data = Array<number>(state.Points.length).fill(NaN, 0, offset).fill(state.Prediction.median, offset)
        chart.data.datasets[5].data = Array<number>(state.Points.length).fill(NaN, 0, offset).fill(state.Prediction.maximal, offset)
        chart.data.datasets[6].data = Array<number>(state.Points.length).fill(NaN, 0, offset).fill(state.Prediction.minimal, offset)

        plot_max = Math.max(plot_max, state.Prediction.maximal);
        //plot_min = Math.max(plot_min, state.Prediction.minimal) - (state.Prediction.maximal - state.Prediction.minimal);
    } else {
        chart.data.datasets[4].data = []
        chart.data.datasets[5].data = []
        chart.data.datasets[6].data = []
    }

    // approx
    chart.data.datasets[7].data = Array<number>(state.Points.length).fill(NaN);
    for (var i = 0; i < state.Aproximations.length; ++i) {
        const d: Array<[number, number]> = state.Aproximations[i];
        const res_index_offset = chart.data.labels.findIndex((v) => v == d[0][0]);
        if (res_index_offset >= 0) {
            for (var j = 0; j < d.length; ++j) {
                chart.data.datasets[7].data[res_index_offset + j] = d[j][1];
            }
        }
        plot_min = Math.min(plot_min, Math.min(...(<number[]>chart.data.datasets[7].data).filter((v) => v !== null && !isNaN(v))))
    }

    // Y-axis limits
    chart.options.scales.yAxes[0].ticks.min = plot_min;
    chart.options.scales.yAxes[0].ticks.max = plot_max;
    chart.update();

    update_f_re_display({
        freq: current_freq,
        min: lowerLimit,
        max: upperLimit
    });

    update_rezonator_table(state);

    update_camera_controls(state.CloseTimestamp, state.Points.pop()[0]);

    update_autoadj_button(state.IsAutoAdjustBusy);
}

function start_updater(chart: Chart) {
    oboe('/state')
        .done((state: IState) => {
            update(chart, state);
            if (state.RestartMarker) {
                setTimeout(() => start_updater(chart), 0)
            }
        })
}