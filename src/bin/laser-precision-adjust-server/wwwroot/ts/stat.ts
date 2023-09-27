
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

        select_row_table(channel_to_select);
    });
});


function select_row_table(row: number): void {
    const primary_class = 'bg-primary';
    const newly_selected = $('#rez-' + row.toString() + '-row');
    if (!newly_selected.hasClass(primary_class)) {
        newly_selected.addClass(primary_class).siblings().removeClass(primary_class);
    }
}