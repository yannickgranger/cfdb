<?php

namespace Enrolment;

interface Labelled
{
    public function label(): string;
}

enum PriorStudy: string implements Labelled
{
    case None = 'none';
    case Some = 'some';

    public function label(): string
    {
        return ucfirst($this->value);
    }
}

enum StudyDuration implements \Vendor\External
{
    case Short;
}

class Enrolment implements Labelled
{
    public function label(): string
    {
        return 'enrolment';
    }
}
