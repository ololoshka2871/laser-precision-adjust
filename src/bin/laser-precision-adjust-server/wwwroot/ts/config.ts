
//-----------------------------------------------------------------------------

// on page loaded jquery
$(() => {
    $('.adj-input').on('input', (ev) => {
        const $target = $(ev.target);
        const parameter = $target.prop('name');
        const value = parseFloat($target.val().toString());

        var data = {};
        data[parameter] = value;
        $.ajax({
            url: '/config-and-save',
            method: 'PATCH',
            data: JSON.stringify(data),
            contentType: 'application/json',
        });
    });
});
